use crate::helpers::common::ClusterDomain;
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use ::function_name::named;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::models::VpcQoveryNetworkMode::WithoutNatGateways;
use qovery_engine::cloud_provider::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::ToCloudProviderFormat;
use qovery_engine::utilities::to_short_id;

#[cfg(feature = "test-gcp-infra")]
fn create_and_destroy_gke_cluster(
    region: GcpRegion,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        let organization_id = generate_organization_id(region.to_string().as_str());
        let zones = region.zones();
        cluster_test(
            test_name,
            Kind::Gcp,
            KKind::Gke,
            context_for_cluster(organization_id, cluster_id, Some(KKind::Gke)),
            logger(),
            metrics_registry(),
            region.to_cloud_provider_format(),
            Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            Option::from(vpc_network_mode),
            CpuArchitecture::AMD64,
            None,
        )
    })
}

#[cfg(feature = "test-gcp-infra")]
#[named]
#[test]
#[ignore]
fn create_and_destroy_gke_cluster_without_nat_gw_in_europe_west_9() {
    let region = GcpRegion::EuropeWest9;
    create_and_destroy_gke_cluster(region, ClusterTestType::Classic, WithoutNatGateways, function_name!());
}
