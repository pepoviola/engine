use crate::helpers::aws_ec2::{ec2_kubernetes_instance, AWS_EC2_KUBERNETES_VERSION};
use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::utilities::{init, FuncTestsSecrets};

use crate::helpers::aws::AWS_KUBERNETES_VERSION;
use crate::helpers::gcp::GCP_KUBERNETES_VERSION;
use crate::helpers::scaleway::SCW_KUBERNETES_VERSION;
use core::option::Option;
use core::option::Option::{None, Some};
use core::result::Result::Err;
use qovery_engine::cloud_provider::aws::kubernetes::ec2::EC2;
use qovery_engine::cloud_provider::aws::kubernetes::eks::EKS;
use qovery_engine::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::gcp::kubernetes::Gke;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, Kubernetes, KubernetesVersion};
use qovery_engine::cloud_provider::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::{CloudProvider, Kind};
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::transaction::{Transaction, TransactionResult};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{span, Level};
use uuid::Uuid;

pub const KUBERNETES_MIN_NODES: i32 = 5;
pub const KUBERNETES_MAX_NODES: i32 = 10;

pub enum ClusterTestType {
    Classic,
    WithPause,
    WithUpgrade,
    WithNodesResize,
}

pub fn get_cluster_test_kubernetes<'a>(
    secrets: FuncTestsSecrets,
    context: &Context,
    cluster_id: String,
    cluster_name: String,
    boot_version: KubernetesVersion,
    localisation: &str,
    aws_zones: Option<Vec<AwsZone>>,
    cloud_provider: Arc<dyn CloudProvider>,
    kubernetes_provider: KubernetesKind,
    dns_provider: Arc<dyn DnsProvider>,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    min_nodes: i32,
    max_nodes: i32,
    cpu_archi: CpuArchitecture,
) -> Box<dyn Kubernetes + 'a> {
    let kubernetes: Box<dyn Kubernetes> = match kubernetes_provider {
        KubernetesKind::Eks => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EKS::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version,
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EC2::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version,
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 7200,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap(),
            )
        }
        KubernetesKind::ScwKapsule => Box::new(
            Kapsule::new(
                context.clone(),
                Uuid::new_v4(),
                cluster_name,
                boot_version,
                ScwZone::from_str(localisation).expect("Unknown zone set for Kapsule"),
                cloud_provider,
                dns_provider,
                Scaleway::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                logger,
                metrics_registry,
                ClusterAdvancedSettings {
                    pleco_resources_ttl: 14400,
                    ..Default::default()
                },
                None,
            )
            .unwrap(),
        ),
        KubernetesKind::Gke => todo!(),            // TODO(benjaminch): GKE integration
        KubernetesKind::EksSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::GkeSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::ScwSelfManaged => todo!(), // TODO: Byok integration
    };

    kubernetes
}

pub fn cluster_test(
    test_name: &str,
    provider_kind: Kind,
    kubernetes_kind: KubernetesKind,
    context: Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    region: &str,
    _zones: Option<Vec<&str>>,
    test_type: ClusterTestType,
    cluster_domain: &ClusterDomain,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    cpu_archi: CpuArchitecture,
    environment_to_deploy: Option<&EnvironmentRequest>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let kubernetes_boot_version = match kubernetes_kind {
        KubernetesKind::Eks | KubernetesKind::EksSelfManaged => AWS_KUBERNETES_VERSION,
        KubernetesKind::Ec2 => AWS_EC2_KUBERNETES_VERSION.clone(),
        KubernetesKind::ScwKapsule | KubernetesKind::ScwSelfManaged => SCW_KUBERNETES_VERSION,
        KubernetesKind::Gke | KubernetesKind::GkeSelfManaged => GCP_KUBERNETES_VERSION,
    };

    let mut engine = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            region,
            kubernetes_kind,
            kubernetes_boot_version.clone(),
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            cpu_archi,
            EngineLocation::ClientSide,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            region,
            kubernetes_kind,
            kubernetes_boot_version.clone(),
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
        ),
        Kind::Gcp => Gke::docker_cr_engine(
            &context,
            logger.clone(),
            metrics_registry.clone(),
            region,
            kubernetes_kind,
            kubernetes_boot_version.clone(),
            cluster_domain,
            vpc_network_mode.clone(),
            i32::MIN, // NA due to GKE autopilot
            i32::MAX, // NA due to GKE autopilot
            CpuArchitecture::AMD64,
            EngineLocation::ClientSide,
        ),
        Kind::SelfManaged => todo!(),
    };
    // Bootstrap
    let mut bootstrap_tx = Transaction::new(&engine).unwrap();
    if let Err(err) = bootstrap_tx.create_kubernetes() {
        panic!("{err:?}")
    }
    assert!(matches!(bootstrap_tx.commit(), TransactionResult::Ok));

    // update
    engine.context_mut().update_is_first_cluster_deployment(false);
    let mut update_tx = Transaction::new(&engine).unwrap();
    if let Err(err) = update_tx.create_kubernetes() {
        panic!("{err:?}")
    }
    assert!(matches!(update_tx.commit(), TransactionResult::Ok));

    // Deploy env if any
    if let Some(env) = environment_to_deploy {
        let mut env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry(),
                engine.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::cloud_provider::service::Action::Create;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, |_| {}, &|| false) {
            panic!("{ret:?}")
        }
    }

    match test_type {
        // TODO new test type
        ClusterTestType::Classic => {}
        ClusterTestType::WithPause => {
            let mut pause_tx = Transaction::new(&engine).unwrap();
            let mut resume_tx = Transaction::new(&engine).unwrap();

            // Pause
            if let Err(err) = pause_tx.pause_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(pause_tx.commit(), TransactionResult::Ok));

            // Resume
            if let Err(err) = resume_tx.create_kubernetes() {
                panic!("{err:?}")
            }

            assert!(matches!(resume_tx.commit(), TransactionResult::Ok));
        }
        ClusterTestType::WithUpgrade => {
            let upgrade_to_version = kubernetes_boot_version.next_version().unwrap_or_else(|| {
                panic!("Kubernetes version `{kubernetes_boot_version}` has no next version defined for now",)
            });
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    region,
                    KubernetesKind::Eks,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    region,
                    KubernetesKind::ScwKapsule,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                ),
                Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
                Kind::SelfManaged => todo!(),
            };
            let mut upgrade_tx = Transaction::new(&engine).unwrap();
            let mut delete_tx = Transaction::new(&engine).unwrap();

            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

            return test_name.to_string();
        }
        ClusterTestType::WithNodesResize => {
            let min_nodes = 11;
            let max_nodes = 15;
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    region,
                    KubernetesKind::Eks,
                    kubernetes_boot_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    metrics_registry.clone(),
                    region,
                    KubernetesKind::ScwKapsule,
                    kubernetes_boot_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    CpuArchitecture::AMD64,
                    EngineLocation::ClientSide,
                ),
                Kind::Gcp => todo!(), // TODO(benjaminch): GKE integration
                Kind::SelfManaged => todo!(),
            };
            let mut upgrade_tx = Transaction::new(&engine).unwrap();
            let mut delete_tx = Transaction::new(&engine).unwrap();
            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));
            return test_name.to_string();
        }
    }

    // Destroy env if any
    if let Some(env) = environment_to_deploy {
        let mut env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry(),
                engine.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::cloud_provider::service::Action::Delete;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, |_| {}, &|| false) {
            panic!("{ret:?}")
        }
    }

    // // Delete
    let mut delete_tx = Transaction::new(&engine).unwrap();
    if let Err(err) = delete_tx.delete_kubernetes() {
        panic!("{err:?}")
    }
    assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

    test_name.to_string()
}

pub fn get_environment_test_kubernetes(
    context: &Context,
    cloud_provider: Arc<dyn CloudProvider>,
    kubernetes_version: KubernetesVersion,
    dns_provider: Arc<dyn DnsProvider>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    localisation: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    min_nodes: i32,
    max_nodes: i32,
    cpu_archi: CpuArchitecture,
    engine_location: EngineLocation,
) -> Box<dyn Kubernetes> {
    let secrets = FuncTestsSecrets::new();

    let kubernetes: Box<dyn Kubernetes> = match cloud_provider.kubernetes_kind() {
        KubernetesKind::Eks => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, engine_location);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EKS::new(
                    context.clone(),
                    context.cluster_short_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        aws_vpc_enable_flow_logs: true,
                        aws_eks_ec2_metadata_imds: qovery_engine::cloud_provider::io::AwsEc2MetadataImds::Required,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EC2::new(
                    context.clone(),
                    context.cluster_short_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 7200,
                        aws_vpc_enable_flow_logs: false,
                        ..Default::default()
                    },
                    None,
                )
                .expect("Cannot instantiate AWS EKS"),
            )
        }
        KubernetesKind::ScwKapsule => {
            let zone = ScwZone::from_str(localisation).expect("SCW zone not supported");
            Box::new(
                Kapsule::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()),
                    kubernetes_version,
                    zone,
                    cloud_provider,
                    dns_provider,
                    Scaleway::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                    Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        ..Default::default()
                    },
                    None,
                )
                .expect("Cannot instantiate SCW Kapsule"),
            )
        }
        KubernetesKind::Gke => {
            let region = GcpRegion::from_str(localisation).expect("GCP zone not supported");
            Box::new(
                Gke::new(
                    context.clone(),
                    context.cluster_short_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region,
                    cloud_provider,
                    dns_provider,
                    Gke::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                    logger,
                    metrics_registry,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        ..Default::default()
                    },
                    None,
                )
                .expect("Cannot instantiate GKE"),
            )
        }
        KubernetesKind::GkeSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::ScwSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::EksSelfManaged => todo!(), // TODO: Byok integration
    };

    kubernetes
}
