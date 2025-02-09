use std::borrow::Borrow;
use std::fs;
use std::sync::Arc;

use base64::engine::general_purpose;
use base64::Engine;
use uuid::Uuid;

use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{self, Kubernetes, KubernetesVersion};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::CloudProvider;
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::object_storage::ObjectStorage;
use crate::secret_manager::vault::QVaultClient;
use serde::{Deserialize, Serialize};

pub struct SelfManaged {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    cloud_provider: Arc<dyn CloudProvider>,
    region: String,
    dns_provider: Arc<dyn DnsProvider>,
    #[allow(dead_code)] //TODO(pmavro): not yet implemented
    options: SelfManagedOptions,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    advanced_settings: ClusterAdvancedSettings,
    kubeconfig: Option<String>,
}

impl SelfManaged {
    pub fn new(
        context: Context,
        id: String,
        long_id: Uuid,
        name: String,
        version: KubernetesVersion,
        cloud_provider: Arc<dyn CloudProvider>,
        dns_provider: Arc<dyn DnsProvider>,
        options: SelfManagedOptions,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        advanced_settings: ClusterAdvancedSettings,
        kubeconfig: Option<String>,
    ) -> Result<SelfManaged, Box<EngineError>> {
        let self_managed = SelfManaged {
            context,
            id,
            long_id,
            name,
            version,
            cloud_provider: cloud_provider.clone(),
            region: cloud_provider.region(),
            dns_provider,
            options,
            logger,
            metrics_registry,
            advanced_settings,
            kubeconfig,
        };
        // create kubeconfig file so it can be used later
        self_managed.create_kubeconfig_from_kubernetes_connection()?;
        Ok(self_managed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfManagedOptions {
    // Qovery
    pub qovery_grpc_url: String,
    #[serde(default)]
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_engine_location: EngineLocation,
}

impl Kubernetes for SelfManaged {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> kubernetes::Kind {
        self.cloud_provider.kubernetes_kind()
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
        self.region.as_str()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        None
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider.as_ref()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider.as_ref()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn metrics_registry(&self) -> &dyn MetricsRegistry {
        self.metrics_registry.borrow()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        todo!()
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        true
    }

    fn cpu_architectures(&self) -> Vec<crate::cloud_provider::models::CpuArchitecture> {
        vec![]
    }

    fn on_create(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn upgrade_with_status(
        &self,
        _kubernetes_upgrade_status: kubernetes::KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_upgrade(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_upgrade_error(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause_error(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn update_vault_config(
        &self,
        event_details: crate::events::EventDetails,
        _qovery_terraform_config_file: String,
        cluster_secrets: crate::cloud_provider::vault::ClusterSecrets,
        kubeconfig_file_path: Option<&std::path::Path>,
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
            let _ = cluster_secrets_update.create_or_update_secret(&vault, false, event_details);
        };
        Ok(())
    }

    fn advanced_settings(&self) -> &crate::cloud_provider::io::ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn customer_helm_charts_override(
        &self,
    ) -> Option<
        std::collections::HashMap<
            crate::io_models::engine_request::ChartValuesOverrideName,
            crate::io_models::engine_request::ChartValuesOverrideValues,
        >,
    > {
        None
    }

    fn get_kubernetes_connection(&self) -> Option<String> {
        self.kubeconfig.clone()
    }
}
