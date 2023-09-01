use crate::events::{EngineMsg, EngineMsgPayload};
use crate::msg_publisher::{MsgPublisher, StdMsgPublisher};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum StepName {
    Total,
    ProvisionBuilder,
    RegistryCreateRepository,
    GitClone,
    Build,
    Deployment,
}

impl ToString for StepName {
    fn to_string(&self) -> String {
        match self {
            StepName::Total => "Total".to_string(),
            StepName::ProvisionBuilder => "ProvisionBuilder".to_string(),
            StepName::RegistryCreateRepository => "RegistryCreateRepository".to_string(),
            StepName::GitClone => "GitClone".to_string(),
            StepName::Build => "Build".to_string(),
            StepName::Deployment => "Deployment".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepLabel {
    Service,
    Environment,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StepStatus {
    Success,
    Error,
    Cancel,
    Skip,
    NotSet,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StepRecord {
    pub step_name: StepName,
    pub label: StepLabel,
    pub id: Uuid,
    start_time: Instant,
    pub duration: Option<Duration>,
    pub status: Option<StepStatus>,
}

#[derive(Clone)]
pub struct StepRecordHandle {
    id: Uuid,
    name: StepName,
    metrics_registry: Box<dyn MetricsRegistry>,
}

pub trait MetricsRegistry: Send + Sync {
    fn start_record(&self, id: Uuid, label: StepLabel, step_name: StepName) -> StepRecordHandle;
    fn stop_record(&self, id: Uuid, deployment_step: StepName, status: StepStatus);
    fn record_is_stopped(&self, id: Uuid, deployment_step: StepName) -> bool;
    fn get_records(&self, service_id: Uuid) -> Vec<StepRecord>;
    fn clear(&self);
    fn clone_dyn(&self) -> Box<dyn MetricsRegistry>;
}

impl Clone for Box<dyn MetricsRegistry> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

impl StepRecord {
    pub fn new(step_name: StepName, label: StepLabel, id: Uuid) -> Self {
        StepRecord {
            step_name,
            label,
            id,
            start_time: Instant::now(),
            duration: None,
            status: None,
        }
    }
}

impl StepRecordHandle {
    pub fn new(id: Uuid, name: StepName, metrics_registry: Box<dyn MetricsRegistry>) -> Self {
        StepRecordHandle {
            id,
            name,
            metrics_registry,
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.metrics_registry.record_is_stopped(self.id, self.name.clone())
    }

    pub fn stop(&self, status: StepStatus) {
        self.metrics_registry.stop_record(self.id, self.name.clone(), status);
    }
}

impl Drop for StepRecordHandle {
    fn drop(&mut self) {
        if !self.is_stopped() {
            self.stop(StepStatus::NotSet)
        }
    }
}

type StepRecordMap = HashMap<StepName, StepRecord>;
type MetricsRegistryMap = HashMap<Uuid, StepRecordMap>;

#[derive(Clone)]
pub struct StdMetricsRegistry {
    registry: Arc<Mutex<MetricsRegistryMap>>,
    message_publisher: Arc<Box<dyn MsgPublisher>>,
}

impl StdMetricsRegistry {
    pub fn new(message_publisher: Box<dyn MsgPublisher>) -> Self {
        StdMetricsRegistry {
            registry: Arc::new(Mutex::new(HashMap::new())),
            message_publisher: Arc::new(message_publisher),
        }
    }
}

impl Default for StdMetricsRegistry {
    fn default() -> Self {
        Self::new(Box::<StdMsgPublisher>::default())
    }
}

impl MetricsRegistry for StdMetricsRegistry {
    fn start_record(&self, id: Uuid, label: StepLabel, step_name: StepName) -> StepRecordHandle {
        debug!("start record deployment step {:#?} for item {}", step_name, id);

        let mut registry = self.registry.lock().unwrap();
        let metrics_per_id = registry.entry(id).or_insert(HashMap::new());

        if metrics_per_id.contains_key(&step_name) {
            error!("key {:#?} already exist", step_name);
        }

        metrics_per_id.insert(step_name.clone(), StepRecord::new(step_name.clone(), label, id));
        StepRecordHandle::new(id, step_name, Box::new(self.clone()))
    }

    fn stop_record(&self, id: Uuid, step_name: StepName, status: StepStatus) {
        debug!("stop record deployment step {:#?} for item {}", step_name, id);

        let mut registry = self.registry.lock().unwrap();
        let metrics_per_id = registry.entry(id).or_insert(HashMap::new());

        if let Some(deployment_step_record) = metrics_per_id.get_mut(&step_name) {
            deployment_step_record.duration = Some(deployment_step_record.start_time.elapsed());
            deployment_step_record.status = Some(status);

            self.message_publisher
                .send(EngineMsg::new(EngineMsgPayload::Metrics(deployment_step_record.clone())));
        } else {
            error!(
                "stop record deployment step {:#?} for service {} that has not been started",
                step_name, id
            );
        }
    }

    fn record_is_stopped(&self, id: Uuid, step_name: StepName) -> bool {
        let mut locked_registry = self.registry.lock().unwrap();
        let metrics_per_id = locked_registry.entry(id).or_insert(HashMap::new());
        if let Some(deployment_step_record) = metrics_per_id.get(&step_name) {
            if deployment_step_record.duration.is_some() {
                return true;
            }
        }
        false
    }

    fn get_records(&self, id: Uuid) -> Vec<StepRecord> {
        debug!("get step durations for item ${}", id);

        let mut registry = self.registry.lock().unwrap();
        let metrics_per_service = registry.entry(id).or_insert(HashMap::new());
        metrics_per_service
            .values()
            .filter(|record| record.duration.is_some())
            .cloned()
            .collect()
    }

    fn clear(&self) {
        debug!("clear the registry");
        let mut registry = self.registry.lock().unwrap();
        registry.clear()
    }

    fn clone_dyn(&self) -> Box<dyn MetricsRegistry> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::metrics_registry::{MetricsRegistry, StdMetricsRegistry, StepLabel, StepName, StepStatus};
    use crate::msg_publisher::StdMsgPublisher;
    use uuid::Uuid;

    #[test]
    fn test_get_records_when_registry_is_empty() {
        let service_id = Uuid::new_v4();
        let metrics_register = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        let record_infos = metrics_register.get_records(service_id);
        assert_eq!(record_infos, vec![]);
    }

    #[test]
    fn test_get_records_when_registry_is_not_empty() {
        let service_id = Uuid::new_v4();
        let step_name = StepName::Deployment;
        let step_label = StepLabel::Service;
        let step_status = StepStatus::Success;
        let metrics_register = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        {
            // to trigger the record drop
            metrics_register.start_record(service_id, step_label, step_name.clone());
            metrics_register.stop_record(service_id, step_name.clone(), step_status.clone());
        }

        let records = metrics_register.get_records(service_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().unwrap().step_name, step_name);
        assert_eq!(records.first().unwrap().id, service_id);
        assert!(records.first().unwrap().duration.is_some());
        assert_eq!(records.first().unwrap().status, Some(step_status));
    }

    #[test]
    fn test_get_records_when_record_is_stopped() {
        let service_id = Uuid::new_v4();
        let step_name = StepName::Deployment;
        let step_label = StepLabel::Service;
        let step_status = StepStatus::Success;
        let metrics_register = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        {
            // to trigger the record drop
            let record = metrics_register.start_record(service_id, step_label, step_name.clone());
            record.stop(step_status.clone());
        }

        let records = metrics_register.get_records(service_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().unwrap().step_name, step_name);
        assert_eq!(records.first().unwrap().id, service_id);
        assert!(records.first().unwrap().duration.is_some());
        assert_eq!(records.first().unwrap().status, Some(step_status));
    }

    #[test]
    fn test_get_records_when_record_is_dropped() {
        let service_id = Uuid::new_v4();
        let step_name = StepName::Deployment;
        let step_label = StepLabel::Service;
        let step_status = StepStatus::NotSet;
        let metrics_register = StdMetricsRegistry::new(Box::new(StdMsgPublisher::new()));

        {
            // to trigger the record drop
            let _record = metrics_register.start_record(service_id, step_label, step_name.clone());
        }

        let records = metrics_register.get_records(service_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().unwrap().step_name, step_name);
        assert_eq!(records.first().unwrap().id, service_id);
        assert!(records.first().unwrap().duration.is_some());
        assert_eq!(records.first().unwrap().status, Some(step_status));
    }
}
