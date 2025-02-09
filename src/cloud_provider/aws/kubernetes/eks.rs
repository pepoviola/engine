use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::models::QoveryAwsSdkConfigEks;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{
    event_details, send_progress_on_long_task, InstanceType, Kind, Kubernetes, KubernetesNodesType,
    KubernetesUpgradeStatus, KubernetesVersion,
};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::CloudProvider;
use crate::cmd::kubectl::{kubectl_exec_scale_replicas, ScalingKind};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::logger::Logger;
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::services::kube_client::SelectK8sResourceBy;
use async_trait::async_trait;
use aws_sdk_eks::error::{
    DeleteNodegroupError, DescribeClusterError, DescribeNodegroupError, ListClustersError, ListNodegroupsError,
};
use aws_types::SdkConfig;

use crate::metrics_registry::MetricsRegistry;
use aws_sdk_eks::output::{
    DeleteNodegroupOutput, DescribeClusterOutput, DescribeNodegroupOutput, ListClustersOutput, ListNodegroupsOutput,
};
use aws_smithy_client::SdkError;

use crate::models::ToCloudProviderFormat;
use base64::engine::general_purpose;
use base64::Engine;
use function_name::named;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

use super::{
    define_cluster_upgrade_timeout, get_rusoto_eks_client, should_update_desired_nodes,
    AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION,
};

/// EKS kubernetes provider allowing to deploy an EKS cluster.
pub struct EKS {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    region: AwsRegion,
    zones: Vec<AwsZone>,
    cloud_provider: Arc<dyn CloudProvider>,
    dns_provider: Arc<dyn DnsProvider>,
    s3: S3,
    nodes_groups: Vec<NodeGroups>,
    template_directory: String,
    options: Options,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    advanced_settings: ClusterAdvancedSettings,
    customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

impl EKS {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: Arc<dyn CloudProvider>,
        dns_provider: Arc<dyn DnsProvider>,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(&*cloud_provider, long_id, name.to_string(), &context);
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;

        // ensure config is ok
        if let Err(e) = EKS::validate_node_groups(nodes_groups.clone(), &event_details) {
            logger.log(EngineEvent::Error(*e.clone(), None));
            return Err(Box::new(*e));
        };
        advanced_settings.validate(event_details)?;

        let s3 = kubernetes::s3(&region, &*cloud_provider);

        // copy listeners from CloudProvider
        Ok(EKS {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            version,
            region,
            zones: aws_zones,
            cloud_provider,
            dns_provider,
            s3,
            options,
            nodes_groups,
            template_directory,
            logger,
            metrics_registry,
            advanced_settings,
            customer_helm_charts_override,
        })
    }

    pub fn validate_node_groups(
        nodes_groups: Vec<NodeGroups>,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        for node_group in &nodes_groups {
            match AwsInstancesType::from_str(node_group.instance_type.as_str()) {
                Ok(x) => {
                    if !EKS::is_instance_allowed(x) {
                        let err = EngineError::new_not_allowed_instance_type(
                            event_details.clone(),
                            node_group.instance_type.as_str(),
                        );
                        return Err(Box::new(err));
                    }
                }
                Err(e) => {
                    let err = EngineError::new_unsupported_instance_type(
                        event_details.clone(),
                        node_group.instance_type.as_str(),
                        e,
                    );
                    return Err(Box::new(err));
                }
            }
        }
        Ok(())
    }

    pub fn is_instance_allowed(instance_type: AwsInstancesType) -> bool {
        instance_type.is_instance_cluster_allowed()
    }

    fn set_cluster_autoscaler_replicas(
        &self,
        event_details: EventDetails,
        replicas_count: u32,
    ) -> Result<(), Box<EngineError>> {
        let autoscaler_new_state = match replicas_count {
            0 => "disable",
            _ => "enable",
        };
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Set cluster autoscaler to: `{autoscaler_new_state}`.")),
        ));
        let kubeconfig_path = self.get_kubeconfig_file()?;
        let selector = "cluster-autoscaler-aws-cluster-autoscaler";
        let namespace = "kube-system";
        kubectl_exec_scale_replicas(
            kubeconfig_path,
            self.cloud_provider().credentials_environment_variables(),
            namespace,
            ScalingKind::Deployment,
            selector,
            replicas_count,
        )
        .map_err(|e| {
            Box::new(EngineError::new_k8s_scale_replicas(
                event_details.clone(),
                selector.to_string(),
                namespace.to_string(),
                replicas_count,
                e,
            ))
        })?;

        Ok(())
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }
}

impl Kubernetes for EKS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Eks
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> KubernetesVersion {
        self.version.clone()
    }

    fn region(&self) -> &str {
        self.region.to_cloud_provider_format()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        Some(self.zones.iter().map(|z| z.to_cloud_provider_format()).collect())
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        (*self.cloud_provider).borrow()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        (*self.dns_provider).borrow()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn metrics_registry(&self) -> &dyn MetricsRegistry {
        self.metrics_registry.borrow()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.s3
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        self.options.user_network_config.is_some()
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        self.nodes_groups.iter().map(|x| x.instance_architecture).collect()
    }

    #[named]
    fn on_create(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || {
            kubernetes::create(
                self,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_create_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::create_error(self))
    }

    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Start preparing EKS cluster upgrade process".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), self) {
            Ok(value) => Some(value),
            Err(_) => None,
        };

        let node_groups_with_desired_states = should_update_desired_nodes(
            event_details.clone(),
            self,
            KubernetesClusterAction::Upgrade(None),
            &self.nodes_groups,
            aws_eks_client,
        )?;

        // in case error, this should no be in the blocking process
        let mut cluster_upgrade_timeout_in_min = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
        if let Ok(kube_client) = self.q_kube_client() {
            let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
                .unwrap_or_else(|_| Vec::with_capacity(0));

            let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
            cluster_upgrade_timeout_in_min = timeout;

            if let Some(x) = message {
                self.logger()
                    .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
            }
        };

        // generate terraform files and copy them into temp dir
        let mut context = kubernetes::tera_context(
            self,
            &self.zones,
            &node_groups_with_desired_states,
            &self.options,
            cluster_upgrade_timeout_in_min,
        )?;

        //
        // Upgrade master nodes
        //
        match &kubernetes_upgrade_status.required_upgrade_on {
            Some(KubernetesNodesType::Masters) => {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Start upgrading process for master nodes.".to_string()),
                ));

                // AWS requires the upgrade to be done in 2 steps (masters, then workers)
                // use the current kubernetes masters' version for workers, in order to avoid migration in one step
                context.insert(
                    "kubernetes_master_version",
                    format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
                );
                // use the current master version for workers, they will be updated later
                context.insert(
                    "eks_workers_version",
                    format!("{}", &kubernetes_upgrade_status.deployed_masters_version).as_str(),
                );

                if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
                    self.template_directory.as_str(),
                    temp_dir.as_str(),
                    context.clone(),
                ) {
                    return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        self.template_directory.to_string(),
                        temp_dir,
                        e,
                    )));
                }

                let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
                let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
                if let Err(e) = crate::template::copy_non_template_files(
                    common_bootstrap_charts.as_str(),
                    common_charts_temp_dir.as_str(),
                ) {
                    return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                        event_details,
                        common_bootstrap_charts,
                        common_charts_temp_dir,
                        e,
                    )));
                }

                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Upgrading Kubernetes master nodes.".to_string()),
                ));

                match terraform_init_validate_plan_apply(
                    temp_dir.as_str(),
                    self.context.is_dry_run_deploy(),
                    self.cloud_provider.credentials_environment_variables().as_slice(),
                ) {
                    Ok(_) => {
                        self.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(
                                "Kubernetes master nodes have been successfully upgraded.".to_string(),
                            ),
                        ));
                    }
                    Err(e) => {
                        return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
                    }
                }
            }
            Some(KubernetesNodesType::Workers) => {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(
                        "No need to perform Kubernetes master upgrade, they are already up to date.".to_string(),
                    ),
                ));
            }
            None => {
                self.logger().log(EngineEvent::Info(
                    event_details,
                    EventMessage::new_from_safe(
                        "No Kubernetes upgrade required, masters and workers are already up to date.".to_string(),
                    ),
                ));
                return Ok(());
            }
        }

        //
        // Upgrade worker nodes
        //
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Preparing workers nodes for upgrade for Kubernetes cluster.".to_string()),
        ));

        // disable cluster autoscaler to avoid interfering with AWS upgrade procedure
        context.insert("enable_cluster_autoscaler", &false);
        context.insert(
            "eks_workers_version",
            format!("{}", &kubernetes_upgrade_status.requested_version).as_str(),
        );

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context.clone(),
        ) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            )));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        if let Err(e) =
            crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
        {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                common_bootstrap_charts,
                common_charts_temp_dir,
                e,
            )));
        }

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Starting Kubernetes worker nodes upgrade".to_string()),
        ));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Checking clusters content health".to_string()),
        ));

        // disable all replicas with issues to avoid upgrade failures
        let kube_client = self.q_kube_client()?;
        let deployments = block_on(kube_client.get_deployments(event_details.clone(), None, SelectK8sResourceBy::All))?;
        for deploy in deployments {
            let status = match deploy.status {
                Some(s) => s,
                None => continue,
            };

            let replicas = status.replicas.unwrap_or(0);
            let ready_replicas = status.ready_replicas.unwrap_or(0);

            // if number of replicas > 0: it is not already disabled
            // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
            if replicas > 0 && ready_replicas == 0 {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "Deployment {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                        deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                    )),
                ));
                block_on(kube_client.set_deployment_replicas_number(
                    event_details.clone(),
                    deploy.metadata.name.as_str(),
                    deploy.metadata.namespace.as_str(),
                    0,
                ))?;
            } else {
                info!(
                    "Deployment {}/{} has {}/{} replicas ready. No action needed.",
                    deploy.metadata.name, deploy.metadata.namespace, ready_replicas, replicas
                );
            }
        }
        // same with statefulsets
        let statefulsets =
            block_on(kube_client.get_statefulsets(event_details.clone(), None, SelectK8sResourceBy::All))?;
        for sts in statefulsets {
            let status = match sts.status {
                Some(s) => s,
                None => continue,
            };

            let ready_replicas = status.ready_replicas.unwrap_or(0);

            // if number of replicas > 0: it is not already disabled
            // ready_replicas == 0: there is something in progress (rolling restart...) so we should not touch it
            if status.replicas > 0 && ready_replicas == 0 {
                self.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "Statefulset {}/{} has {}/{} replicas ready. Scaling to 0 replicas to avoid upgrade failure.",
                        sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                    )),
                ));
                block_on(kube_client.set_statefulset_replicas_number(
                    event_details.clone(),
                    sts.metadata.name.as_str(),
                    sts.metadata.namespace.as_str(),
                    0,
                ))?;
            } else {
                info!(
                    "Statefulset {}/{} has {}/{} replicas ready. No action needed.",
                    sts.metadata.name, sts.metadata.namespace, ready_replicas, status.replicas
                );
            }
        }

        if let Err(e) = self.delete_crashlooping_pods(
            None,
            None,
            Some(3),
            self.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(*e.clone(), None));
            return Err(e);
        }

        if let Err(e) = self.delete_completed_jobs(
            self.cloud_provider().credentials_environment_variables(),
            Infrastructure(InfrastructureStep::Upgrade),
        ) {
            self.logger().log(EngineEvent::Error(*e.clone(), None));
            return Err(e);
        }

        // Disable cluster autoscaler deployment and be sure we re-enable it on exist
        let ev = event_details.clone();
        let _guard = scopeguard::guard(self.set_cluster_autoscaler_replicas(event_details.clone(), 0)?, |_| {
            let _ = self.set_cluster_autoscaler_replicas(ev, 1);
        });

        terraform_init_validate_plan_apply(
            temp_dir.as_str(),
            self.context.is_dry_run_deploy(),
            self.cloud_provider().credentials_environment_variables().as_slice(),
        )
        .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

        self.check_workers_on_upgrade(kubernetes_upgrade_status.requested_version.to_string())
            .map_err(|e| EngineError::new_k8s_node_not_ready(event_details.clone(), e))?;

        self.logger().log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
        ));

        Ok(())
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade())
    }

    #[named]
    fn on_upgrade_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::upgrade_error(self))
    }

    #[named]
    fn on_pause(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || {
            kubernetes::pause(
                self,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || kubernetes::pause_error(self))
    }

    #[named]
    fn on_delete(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || {
            kubernetes::delete(
                self,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
            )
        })
    }

    #[named]
    fn on_delete_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || kubernetes::delete_error(self))
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        _qovery_terraform_config_file: String,
        cluster_secrets: crate::cloud_provider::vault::ClusterSecrets,
        kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>> {
        let vault_conn = match QVaultClient::new(event_details.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        if let Some(vault) = vault_conn {
            // encode base64 kubeconfig
            let kubeconfig = match kubeconfig_file_path {
                Some(x) => fs::read_to_string(x)
                    .map_err(|e| {
                        EngineError::new_cannot_retrieve_cluster_config_file(
                            event_details.clone(),
                            CommandError::new_from_safe_message(format!(
                                "Cannot read kubeconfig file {}: {e}",
                                x.to_str().unwrap_or_default()
                            )),
                        )
                    })
                    .expect("kubeconfig was not found while it should be present"),
                None => self.get_kubeconfig_file()?.to_str().unwrap_or_default().to_string(),
            };
            let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);

            let mut cluster_secrets_update = cluster_secrets;
            cluster_secrets_update.set_kubeconfig_b64(kubeconfig_b64);

            // update info without taking care of the kubeconfig because we don't have it yet
            let _ = cluster_secrets_update.create_or_update_secret(&vault, false, event_details.clone());
        };

        Ok(())
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn customer_helm_charts_override(&self) -> Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>> {
        self.customer_helm_charts_override.clone()
    }

    fn get_kubernetes_connection(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
impl NodeGroupsWithDesiredState {
    fn new(
        name: String,
        id: Option<String>,
        min_nodes: i32,
        max_nodes: i32,
        desired_size: i32,
        enable_desired_size: bool,
        instance_type: String,
        disk_size_in_gib: i32,
    ) -> NodeGroupsWithDesiredState {
        NodeGroupsWithDesiredState {
            name,
            id,
            min_nodes,
            max_nodes,
            desired_size,
            enable_desired_size,
            instance_type,
            disk_size_in_gib,
            instance_architecture: CpuArchitecture::AMD64,
        }
    }
}

pub fn select_nodegroups_autoscaling_group_behavior(
    action: KubernetesClusterAction,
    nodegroup: &NodeGroups,
) -> NodeGroupsWithDesiredState {
    let nodegroup_desired_state = |x| {
        // desired nodes can't be lower than min nodes
        if x < nodegroup.min_nodes {
            (true, nodegroup.min_nodes)
        // desired nodes can't be higher than max nodes
        } else if x > nodegroup.max_nodes {
            (true, nodegroup.max_nodes)
        } else {
            (false, x)
        }
    };

    match action {
        KubernetesClusterAction::Bootstrap => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, true)
        }
        KubernetesClusterAction::Update(current_nodes) | KubernetesClusterAction::Upgrade(current_nodes) => {
            let (upgrade_required, desired_state) = match current_nodes {
                Some(x) => nodegroup_desired_state(x),
                // if nothing is given, it's may be because the nodegroup has been deleted manually, so if we need to set it otherwise we won't be able to create a new nodegroup
                None => (true, nodegroup.max_nodes),
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, desired_state, upgrade_required)
        }
        KubernetesClusterAction::Pause | KubernetesClusterAction::Delete => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, false)
        }
        KubernetesClusterAction::Resume(current_nodes) => {
            // we always want to set the desired sate here to optimize the speed to return to the best situation
            // TODO: (pmavro) save state on pause and reread it on resume
            let resume_nodes_number = match current_nodes {
                Some(x) => nodegroup_desired_state(x).1,
                None => nodegroup.min_nodes,
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, resume_nodes_number, true)
        }
    }
}

#[async_trait]
impl QoveryAwsSdkConfigEks for SdkConfig {
    async fn list_clusters(&self) -> Result<ListClustersOutput, SdkError<ListClustersError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_clusters().send().await
    }

    async fn describe_cluster(
        &self,
        cluster_id: String,
    ) -> Result<DescribeClusterOutput, SdkError<DescribeClusterError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.describe_cluster().name(cluster_id).send().await
    }

    async fn list_all_eks_nodegroups(
        &self,
        cluster_name: String,
    ) -> Result<ListNodegroupsOutput, SdkError<ListNodegroupsError>> {
        let client = aws_sdk_eks::Client::new(self);
        client.list_nodegroups().cluster_name(cluster_name).send().await
    }

    async fn describe_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_id: String,
    ) -> Result<DescribeNodegroupOutput, SdkError<DescribeNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .describe_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_id)
            .send()
            .await
    }

    async fn describe_nodegroups(
        &self,
        cluster_name: String,
        nodegroups: ListNodegroupsOutput,
    ) -> Result<Vec<DescribeNodegroupOutput>, SdkError<DescribeNodegroupError>> {
        let mut nodegroups_descriptions = Vec::new();

        for nodegroup in nodegroups.nodegroups.unwrap_or_default() {
            let nodegroup_description = self.describe_nodegroup(cluster_name.clone(), nodegroup).await;
            match nodegroup_description {
                Ok(x) => nodegroups_descriptions.push(x),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(nodegroups_descriptions)
    }

    async fn delete_nodegroup(
        &self,
        cluster_name: String,
        nodegroup_name: String,
    ) -> Result<DeleteNodegroupOutput, SdkError<DeleteNodegroupError>> {
        let client = aws_sdk_eks::Client::new(self);
        client
            .delete_nodegroup()
            .cluster_name(cluster_name)
            .nodegroup_name(nodegroup_name)
            .send()
            .await
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum NodeGroupToRemoveFailure {
    #[error("No cluster found")]
    ClusterNotFound,
    #[error("No nodegroup found for this cluster")]
    NodeGroupNotFound,
    #[error("At lease one nodegroup must be active, no one can be deleted")]
    OneNodeGroupMustBeActiveAtLeast,
}

#[derive(PartialEq, Eq)]
pub enum NodeGroupsDeletionType {
    All,
    FailedOnly,
}

pub async fn delete_eks_nodegroups(
    aws_conn: SdkConfig,
    cluster_name: String,
    is_first_install: bool,
    nodegroup_delete_selection: NodeGroupsDeletionType,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let clusters = match aws_conn.list_clusters().await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_cannot_list_clusters_error(
                event_details.clone(),
                CommandError::new("Couldn't list clusters from AWS".to_string(), Some(e.to_string()), None),
            )))
        }
    };

    if !clusters
        .clusters()
        .unwrap_or_default()
        .iter()
        .any(|x| x == &cluster_name)
    {
        return Err(Box::new(EngineError::new_cannot_get_cluster_error(
            event_details.clone(),
            CommandError::new_from_safe_message(NodeGroupToRemoveFailure::ClusterNotFound.to_string()),
        )));
    };

    let all_cluster_nodegroups = match aws_conn.list_all_eks_nodegroups(cluster_name.clone()).await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new_from_safe_message(e.to_string()),
            )))
        }
    };

    let all_cluster_nodegroups_described = match aws_conn
        .describe_nodegroups(cluster_name.clone(), all_cluster_nodegroups)
        .await
    {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                event_details,
                e.to_string(),
            )))
        }
    };

    // If it is the first installation of the cluster, we dont want to keep any nodegroup.
    // So just delete everything
    let nodegroups_to_delete = if is_first_install || nodegroup_delete_selection == NodeGroupsDeletionType::All {
        info!("Deleting all nodegroups of this cluster as it is the first installation.");
        all_cluster_nodegroups_described
    } else {
        match check_failed_nodegroups_to_remove(all_cluster_nodegroups_described.clone()) {
            Ok(x) => x,
            Err(e) => {
                // print AWS nodegroup errors to the customer (useful when quota is reached)
                if e == NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast {
                    let nodegroup_health_message = all_cluster_nodegroups_described
                        .iter()
                        .map(|n| match n.nodegroup() {
                            Some(nodegroup) => {
                                let nodegroup_name = nodegroup.nodegroup_name().unwrap_or("unknown_nodegroup_name");
                                let nodegroup_status = match nodegroup.health() {
                                    Some(x) =>
                                        x
                                            .issues()
                                            .unwrap_or_default()
                                            .iter()
                                            .map(|x| format!("{:?}: {}", x.code(), x.message().unwrap_or("no AWS specific message given, please contact Qovery and AWS support regarding this nodegroup issue")))
                                            .collect::<Vec<String>>()
                                            .join(", "),
                                    None => "can't get nodegroup status from cloud provider".to_string(),
                                };
                                format!("Nodegroup {nodegroup_name} health is: {nodegroup_status}")
                            }
                            None => "".to_string(),
                        })
                        .collect::<Vec<String>>()
                        .join("\n");

                    return Err(Box::new(EngineError::new_nodegroup_delete_any_nodegroup_error(
                        event_details,
                        nodegroup_health_message,
                    )));
                };

                return Err(Box::new(EngineError::new_nodegroup_delete_error(
                    event_details,
                    None,
                    e.to_string(),
                )));
            }
        }
    };

    for nodegroup in nodegroups_to_delete {
        let nodegroup_name = match nodegroup.nodegroup() {
            Some(x) => x.nodegroup_name().unwrap_or("unknown_nodegroup_name"),
            None => {
                return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                    event_details,
                    format!("{nodegroup:?}"),
                )))
            }
        };

        if let Err(e) = aws_conn
            .delete_nodegroup(cluster_name.clone(), nodegroup_name.to_string())
            .await
        {
            return Err(Box::new(EngineError::new_nodegroup_delete_error(
                event_details,
                Some(nodegroup_name.to_string()),
                e.to_string(),
            )));
        }
    }

    Ok(())
}

fn check_failed_nodegroups_to_remove(
    nodegroups: Vec<DescribeNodegroupOutput>,
) -> Result<Vec<DescribeNodegroupOutput>, NodeGroupToRemoveFailure> {
    let mut failed_nodegroups_to_remove = Vec::new();

    for nodegroup in nodegroups.iter() {
        match nodegroup.nodegroup() {
            Some(ng) => match ng.status() {
                Some(s) => match s {
                    aws_sdk_eks::model::NodegroupStatus::CreateFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::model::NodegroupStatus::DeleteFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::model::NodegroupStatus::Degraded => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    _ => {
                        info!(
                            "Nodegroup {} is in state {:?}, it will not be deleted",
                            ng.nodegroup_name().unwrap_or("unknown name"),
                            s
                        );
                        continue;
                    }
                },
                None => continue,
            },
            None => return Err(NodeGroupToRemoveFailure::NodeGroupNotFound),
        }
    }

    // ensure we don't remove all nodegroups (even failed ones) to avoid blackout
    if failed_nodegroups_to_remove.len() == nodegroups.len() && !nodegroups.is_empty() {
        return Err(NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast);
    }

    Ok(failed_nodegroups_to_remove)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::eks::{
        select_nodegroups_autoscaling_group_behavior, NodeGroupToRemoveFailure, EKS,
    };
    use crate::cloud_provider::models::{
        CpuArchitecture, KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState,
    };
    use crate::errors::Tag;
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use aws_sdk_eks::model::{nodegroup, NodegroupStatus};
    use aws_sdk_eks::output::DescribeNodegroupOutput;
    use uuid::Uuid;

    use super::check_failed_nodegroups_to_remove;

    #[test]
    fn test_nodegroup_failure_deletion() {
        let nodegroup_ok = nodegroup::Builder::default()
            .set_nodegroup_name(Some("nodegroup_ok".to_string()))
            .set_status(Some(NodegroupStatus::Active))
            .build();
        let nodegroup_create_failed = nodegroup::Builder::default()
            .set_nodegroup_name(Some("nodegroup_create_failed".to_string()))
            .set_status(Some(NodegroupStatus::CreateFailed))
            .build();

        // 2 nodegroups, 2 ok => nothing to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
        ];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // 2 nodegroups, 1 ok, 1 create failed => 1 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 1);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );

        // 2 nodegroups, 2 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // 1 nodegroup, 1 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![DescribeNodegroupOutput::builder()
            .nodegroup(nodegroup_create_failed.clone())
            .build()];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // no nodegroups => ok
        let ngs = vec![];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // x nodegroups, 1 ok, 2 create failed, 1 delete failure, others in other states => 4 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder().nodegroup(nodegroup_ok).build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed)
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::CreateFailed)))
                        .set_status(Some(NodegroupStatus::CreateFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Creating)))
                        .set_status(Some(NodegroupStatus::Creating))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Degraded)))
                        .set_status(Some(NodegroupStatus::Degraded))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::DeleteFailed)))
                        .set_status(Some(NodegroupStatus::DeleteFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    nodegroup::Builder::default()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Updating)))
                        .set_status(Some(NodegroupStatus::Updating))
                        .build(),
                )
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 4);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );
        assert_eq!(
            failed_ngs[1].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_CreateFailed"
        );
        assert_eq!(
            failed_ngs[2].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_Degraded"
        );
        assert_eq!(
            failed_ngs[3].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_DeleteFailed"
        );
    }

    #[test]
    fn test_nodegroup_autoscaling_group() {
        let nodegroup_with_ds = |desired_nodes, enable_desired_nodes| {
            NodeGroupsWithDesiredState::new(
                "nodegroup".to_string(),
                None,
                3,
                10,
                desired_nodes,
                enable_desired_nodes,
                "t1000.xlarge".to_string(),
                20,
            )
        };
        let nodegroup = NodeGroups::new(
            "nodegroup".to_string(),
            3,
            10,
            "t1000.xlarge".to_string(),
            20,
            CpuArchitecture::AMD64,
        )
        .unwrap();

        // bootstrap
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Bootstrap, &nodegroup),
            nodegroup_with_ds(3, true) // need true because it's required from AWS to set desired node when initializing the autoscaler
        );
        // pause
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Pause, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // delete
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Delete, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // resume
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(Some(5)), &nodegroup),
            nodegroup_with_ds(5, true)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(None), &nodegroup),
            // if no info is given during resume, we should take the max and let the autoscaler reduce afterwards
            // but by setting it to the max, some users with have to ask support to raise limits
            // also useful when a customer wants to try Qovery, and do not need to ask AWS support in the early phase
            nodegroup_with_ds(3, true)
        );
        // update (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(6)), &nodegroup),
            nodegroup_with_ds(6, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );
        // upgrade (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Upgrade(Some(7)), &nodegroup),
            nodegroup_with_ds(7, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );

        // test autocorrection of silly stuffs
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1)), &nodegroup),
            nodegroup_with_ds(3, true) // set to minimum if desired is below min
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1000)), &nodegroup),
            nodegroup_with_ds(10, true) // set to max if desired is above max
        );
    }

    #[test]
    fn test_allowed_eks_nodes() {
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            Uuid::new_v4().to_string(),
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(Uuid::new_v4(), "".to_string()),
        );
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.medium".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.medium".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3.large".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert!(EKS::validate_node_groups(
            vec![NodeGroups::new("".to_string(), 3, 5, "t3a.large".to_string(), 20, CpuArchitecture::AMD64).unwrap()],
            &event_details,
        )
        .is_ok());
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t3.small".to_string(), 20, CpuArchitecture::AMD64).unwrap()
                ],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t3a.small".to_string(), 20, CpuArchitecture::AMD64).unwrap()
                ],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::NotAllowedInstanceType
        );
        assert_eq!(
            EKS::validate_node_groups(
                vec![
                    NodeGroups::new("".to_string(), 3, 5, "t1000.terminator".to_string(), 20, CpuArchitecture::AMD64)
                        .unwrap()
                ],
                &event_details
            )
            .unwrap_err()
            .tag(),
            &Tag::UnsupportedInstanceType
        );
    }
}
