use crate::build_platform::Build;
use crate::cloud_provider::io::RegistryMirroringMode;
use crate::cloud_provider::models::{
    EnvironmentVariable, InvalidPVCStorage, InvalidStatefulsetStorage, MountedFile, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{get_service_statefulset_name_and_volumes, Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::Protocol::{TCP, UDP};
use crate::io_models::application::{Port, Protocol};
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::models::probe::Probe;
use crate::models::registry_image_source::RegistryImageSource;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use crate::unit_conversion::extract_volume_size;
use crate::utilities::to_short_id;
use itertools::Itertools;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use serde::Serialize;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::time::Duration;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum ContainerError {
    #[error("Container invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Container<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) kube_name: String,
    pub(super) action: Action,
    pub source: RegistryImageSource,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) cpu_request_in_mili: u32,
    pub(super) cpu_limit_in_mili: u32,
    pub(super) ram_request_in_mib: u32,
    pub(super) ram_limit_in_mib: u32,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) public_domain: String,
    pub(super) ports: Vec<Port>,
    pub(super) storages: Vec<Storage<T::StorageTypes>>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: BTreeSet<MountedFile>,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
    pub(super) advanced_settings: ContainerAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
}

pub fn get_mirror_repository_name(
    service_id: &Uuid,
    cluster_id: &Uuid,
    registry_mirroring_mode: &RegistryMirroringMode,
) -> String {
    match registry_mirroring_mode {
        RegistryMirroringMode::Cluster => format!("qovery-mirror-cluster-{cluster_id}"),
        RegistryMirroringMode::Service => format!("qovery-mirror-{service_id}"),
    }
}

pub fn to_public_l4_ports<'a>(
    ports: impl Iterator<Item = &'a Port>,
    protocol: Protocol,
    public_domain: &str,
) -> Option<PublicL4Ports> {
    let ports: Vec<Port> = ports
        .filter(|p| p.publicly_accessible && p.protocol == protocol)
        .cloned()
        .collect();
    if ports.is_empty() {
        None
    } else {
        Some(PublicL4Ports {
            protocol,
            hostnames: ports.iter().map(|p| format!("{}-{}", p.name, public_domain)).collect(),
            ports,
        })
    }
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Container<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        registry_image_source: RegistryImageSource,
        command_args: Vec<String>,
        entrypoint: Option<String>,
        cpu_request_in_mili: u32,
        cpu_limit_in_mili: u32,
        ram_request_in_mib: u32,
        ram_limit_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        public_domain: String,
        ports: Vec<Port>,
        storages: Vec<Storage<T::StorageTypes>>,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        advanced_settings: ContainerAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, ContainerError> {
        if min_instances > max_instances {
            return Err(ContainerError::InvalidConfig(
                "min_instances must be less or equal to max_instances".to_string(),
            ));
        }

        if min_instances == 0 {
            return Err(ContainerError::InvalidConfig(
                "min_instances must be greater than 0".to_string(),
            ));
        }

        if cpu_request_in_mili > cpu_limit_in_mili {
            return Err(ContainerError::InvalidConfig(
                "cpu_request_in_mili must be less or equal to cpu_limit_in_mili".to_string(),
            ));
        }

        if cpu_request_in_mili == 0 {
            return Err(ContainerError::InvalidConfig(
                "cpu_request_in_mili must be greater than 0".to_string(),
            ));
        }

        if ram_request_in_mib > ram_limit_in_mib {
            return Err(ContainerError::InvalidConfig(
                "ram_request_in_mib must be less or equal to ram_limit_in_mib".to_string(),
            ));
        }

        if ram_request_in_mib == 0 {
            return Err(ContainerError::InvalidConfig(
                "ram_request_in_mib must be greater than 0".to_string(),
            ));
        }

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("containers/{long_id}"),
        )
        .map_err(|_| ContainerError::InvalidConfig("Can't create workspace directory".to_string()))?;

        let event_details = mk_event_details(Transmitter::Container(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            name,
            kube_name,
            source: registry_image_source,
            command_args,
            entrypoint,
            cpu_request_in_mili,
            cpu_limit_in_mili,
            ram_request_in_mib,
            ram_limit_in_mib,
            min_instances,
            max_instances,
            public_domain,
            ports,
            storages,
            environment_variables,
            mounted_files,
            readiness_probe,
            liveness_probe,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.kube_label_selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("container-{}", self.long_id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-container", self.lib_root_directory)
    }

    pub fn registry(&self) -> &Registry {
        &self.source.registry
    }

    fn public_ports(&self) -> impl Iterator<Item = &Port> + '_ {
        self.ports.iter().filter(|port| port.publicly_accessible)
    }

    pub(super) fn default_tera_context(&self, target: &DeploymentTarget) -> ContainerTeraContext {
        let environment = &target.environment;
        let kubernetes = &target.kubernetes;
        let registry_info = target.container_registry.registry_info();
        let ctx = ContainerTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            cluster: ClusterTeraContext {
                long_id: *kubernetes.long_id(),
                name: kubernetes.name().to_string(),
                region: kubernetes.region().to_string(),
                zone: kubernetes.default_zone().to_string(),
            },
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                r#type: "container",
                name: self.kube_name().to_string(),
                user_unsafe_name: self.name.clone(),
                // FIXME: We mirror images to cluster private registry
                image_full: format!(
                    "{}/{}:{}",
                    registry_info.endpoint.host_str().unwrap_or_default(),
                    (registry_info.get_image_name)(&get_mirror_repository_name(
                        self.long_id(),
                        kubernetes.long_id(),
                        &kubernetes.advanced_settings().registry_mirroring_mode,
                    )),
                    self.source.tag_for_mirror(&self.long_id)
                ),
                image_tag: self.source.tag_for_mirror(&self.long_id),
                version: self.service_version(),
                command_args: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_mili: format!("{}m", self.cpu_request_in_mili),
                cpu_limit_in_mili: format!("{}m", self.cpu_limit_in_mili),
                ram_request_in_mib: format!("{}Mi", self.ram_request_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.ram_limit_in_mib),
                min_instances: self.min_instances,
                max_instances: self.max_instances,
                public_domain: self.public_domain.clone(),
                ports: self.ports.clone(),
                ports_layer4_public: {
                    let mut vec = Vec::with_capacity(2);
                    if let Some(tcp) = to_public_l4_ports(self.ports.iter(), TCP, &self.public_domain) {
                        vec.push(tcp);
                    }
                    if let Some(udp) = to_public_l4_ports(self.ports.iter(), UDP, &self.public_domain) {
                        vec.push(udp);
                    }
                    vec
                },
                default_port: self.ports.iter().find_or_first(|p| p.is_default).cloned(),
                storages: vec![],
                readiness_probe: self.readiness_probe.clone(),
                liveness_probe: self.liveness_probe.clone(),
                advanced_settings: self.advanced_settings.clone(),
                legacy_deployment_matchlabels: false,
                legacy_volumeclaim_template: false,
                legacy_deployment_from_scaleway: false,
            },
            registry: registry_info
                .registry_docker_json_config
                .as_ref()
                .map(|docker_json| RegistryTeraContext {
                    secret_name: format!("{}-registry", self.kube_name()),
                    docker_json_config: Some(docker_json.to_string()),
                }),
            environment_variables: self.environment_variables.clone(),
            mounted_files: self.mounted_files.clone().into_iter().collect::<Vec<_>>(),
            resource_expiration_in_seconds: Some(kubernetes.advanced_settings().pleco_resources_ttl),
            loadbalancer_l4_annotations: T::loadbalancer_l4_annotations(),
        };

        ctx
    }

    pub fn is_stateful(&self) -> bool {
        !self.storages.is_empty()
    }

    pub fn service_type(&self) -> ServiceType {
        ServiceType::Container
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn publicly_accessible(&self) -> bool {
        self.public_ports().count() > 0
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn kube_legacy_label_selector(&self) -> String {
        format!("appId={}", self.id)
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }

    fn service_version(&self) -> String {
        format!("{}:{}", self.source.image, self.source.tag)
    }
}

impl<T: CloudProvider> Service for Container<T> {
    fn service_type(&self) -> ServiceType {
        self.service_type()
    }

    fn id(&self) -> &str {
        self.id()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn version(&self) -> String {
        self.service_version()
    }

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        self.environment_variables.clone()
    }

    fn get_passwords(&self) -> Vec<String> {
        if let Some(password) = self.source.registry.get_url_with_credentials().password() {
            let decoded_password = urlencoding::decode(password).ok().map(|decoded| decoded.to_string());

            if let Some(decoded) = decoded_password {
                vec![password.to_string(), decoded]
            } else {
                vec![password.to_string()]
            }
        } else {
            vec![]
        }
    }
}

pub trait ContainerService: Service + DeploymentAction + ToTeraContext + Send {
    fn public_ports(&self) -> Vec<&Port>;
    fn advanced_settings(&self) -> &ContainerAdvancedSettings;
    fn image_full(&self) -> String;
    fn startup_timeout(&self) -> Duration;
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl<T: CloudProvider> ContainerService for Container<T>
where
    Container<T>: Service + ToTeraContext + DeploymentAction,
{
    fn public_ports(&self) -> Vec<&Port> {
        self.public_ports().collect_vec()
    }

    fn advanced_settings(&self) -> &ContainerAdvancedSettings {
        &self.advanced_settings
    }

    fn image_full(&self) -> String {
        format!(
            "{}{}:{}",
            self.source.registry.url().to_string().trim_start_matches("https://"),
            self.source.image,
            self.source.tag
        )
    }

    fn startup_timeout(&self) -> Duration {
        let readiness_probe_timeout = if let Some(p) = &self.readiness_probe {
            p.initial_delay_seconds + ((p.timeout_seconds + p.period_seconds) * p.failure_threshold)
        } else {
            60 * 5
        };

        let liveness_probe_timeout = if let Some(p) = &self.liveness_probe {
            p.initial_delay_seconds + ((p.timeout_seconds + p.period_seconds) * p.failure_threshold)
        } else {
            60 * 5
        };

        let probe_timeout = std::cmp::max(readiness_probe_timeout, liveness_probe_timeout);
        let startup_timeout = std::cmp::max(probe_timeout /* * 10 rolling restart percent */, 60 * 10);
        Duration::from_secs(startup_timeout as u64)
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ClusterTeraContext {
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) region: String,
    pub(super) zone: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct PublicL4Ports {
    pub protocol: Protocol,
    pub ports: Vec<Port>,
    pub hostnames: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ServiceTeraContext {
    pub(super) short_id: String,
    pub(super) long_id: Uuid,
    pub(super) r#type: &'static str,
    pub(super) name: String,
    pub(super) user_unsafe_name: String,
    pub(super) image_full: String,
    pub(super) image_tag: String,
    pub(super) version: String,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) cpu_request_in_mili: String,
    pub(super) cpu_limit_in_mili: String,
    pub(super) ram_request_in_mib: String,
    pub(super) ram_limit_in_mib: String,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) public_domain: String,
    pub(super) ports: Vec<Port>,
    pub(super) ports_layer4_public: Vec<PublicL4Ports>,
    pub(super) default_port: Option<Port>,
    pub(super) storages: Vec<StorageDataTemplate>,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
    pub(super) advanced_settings: ContainerAdvancedSettings,
    pub(super) legacy_deployment_matchlabels: bool,
    pub(super) legacy_volumeclaim_template: bool,
    pub(super) legacy_deployment_from_scaleway: bool,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct RegistryTeraContext {
    pub(super) secret_name: String,
    pub(super) docker_json_config: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ContainerTeraContext {
    pub(super) organization_long_id: Uuid,
    pub(super) project_long_id: Uuid,
    pub(super) environment_short_id: String,
    pub(super) environment_long_id: Uuid,
    pub(super) cluster: ClusterTeraContext,
    pub(super) namespace: String,
    pub(super) service: ServiceTeraContext,
    pub(super) registry: Option<RegistryTeraContext>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: Vec<MountedFile>,
    pub(super) resource_expiration_in_seconds: Option<i32>,
    pub(super) loadbalancer_l4_annotations: &'static [(&'static str, &'static str)],
}

pub fn get_container_with_invalid_storage_size<T: CloudProvider>(
    container: &Container<T>,
    kube_client: &kube::Client,
    namespace: &str,
    event_details: &EventDetails,
) -> Result<Option<InvalidStatefulsetStorage>, Box<EngineError>> {
    match !container.is_stateful() {
        true => Ok(None),
        false => {
            let selector = Container::kube_label_selector(container);
            let (statefulset_name, statefulset_volumes) =
                get_service_statefulset_name_and_volumes(kube_client, namespace, &selector, event_details)?;
            let storage_err = Box::new(EngineError::new_service_missing_storage(
                event_details.clone(),
                &container.long_id,
            ));
            let volumes = match statefulset_volumes {
                None => return Err(storage_err),
                Some(volumes) => volumes,
            };
            let mut invalid_storage = InvalidStatefulsetStorage {
                service_type: Container::service_type(container),
                service_id: container.long_id,
                statefulset_selector: selector,
                statefulset_name,
                invalid_pvcs: vec![],
            };

            for volume in volumes {
                if let Some(spec) = &volume.spec {
                    if let Some(resources) = &spec.resources {
                        if let (Some(requests), Some(volume_name)) = (&resources.requests, &volume.metadata.name) {
                            // in order to compare volume size from engine request to effective size in kube, we must get the  effective size
                            let size = extract_volume_size(requests["storage"].0.to_string()).map_err(|e| {
                                Box::new(EngineError::new_cannot_parse_string(
                                    event_details.clone(),
                                    &requests["storage"].0,
                                    e,
                                ))
                            })?;
                            if let Some(storage) = container
                                .storages
                                .iter()
                                .find(|storage| volume_name == &storage.long_id.to_string())
                            {
                                if storage.size_in_gib > size {
                                    // if volume size in request is bigger than effective size we get related PVC to get its infos
                                    if let Some(pvc) =
                                        block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
                                            kube_client,
                                            namespace,
                                            &format!("qovery.com/disk-id={}", storage.long_id),
                                        ))
                                        .map_err(|e| {
                                            EngineError::new_k8s_cannot_get_pvcs(event_details.clone(), namespace, e)
                                        })?
                                        .items
                                        .first()
                                    {
                                        if let Some(pvc_name) = &pvc.metadata.name {
                                            invalid_storage.invalid_pvcs.push(InvalidPVCStorage {
                                                pvc_name: pvc_name.to_string(),
                                                required_disk_size_in_gib: storage.size_in_gib,
                                            })
                                        }
                                    };
                                }

                                if storage.size_in_gib < size {
                                    return Err(Box::new(EngineError::new_invalid_engine_payload(
                                        event_details.clone(),
                                        format!(
                                            "new storage size ({}) should be equal or greater than actual size ({})",
                                            storage.size_in_gib, size
                                        )
                                        .as_str(),
                                        None,
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            match invalid_storage.invalid_pvcs.is_empty() {
                true => Ok(None),
                false => Ok(Some(invalid_storage)),
            }
        }
    }
}
