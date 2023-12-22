use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::job::Job;
use crate::models::types::{ToTeraContext, GCP};
use tera::Context as TeraContext;

impl ToTeraContext for Job<GCP> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        Ok(TeraContext::from_serialize(self.default_tera_context(target)).unwrap_or_default())
    }
}
