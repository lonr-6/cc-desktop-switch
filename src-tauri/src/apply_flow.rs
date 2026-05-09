use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::desktop_writer::DesktopConfigProbe;
use crate::desktop_writer::DesktopWriteResult;
use crate::gateway::GatewayHealth;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApplyResult {
    pub mode: String,
    pub success: bool,
    pub gateway: Option<GatewayHealth>,
    pub desktop_config: Option<DesktopConfigProbe>,
    pub write: Option<DesktopWriteResult>,
    pub steps: Vec<DesktopApplyStep>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApplyStep {
    pub id: String,
    pub label: String,
    pub status: DesktopApplyStepStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopApplyStepStatus {
    Pending,
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyLocalConfigRequest {
    pub config_library_root: PathBuf,
}

impl DesktopApplyResult {
    pub fn new() -> Self {
        Self {
            mode: "local_gateway".to_owned(),
            success: false,
            gateway: None,
            desktop_config: None,
            write: None,
            steps: Vec::new(),
            error: None,
        }
    }

    pub fn push_step(
        &mut self,
        id: &str,
        label: &str,
        status: DesktopApplyStepStatus,
        error: Option<String>,
    ) {
        self.steps.push(DesktopApplyStep {
            id: id.to_owned(),
            label: label.to_owned(),
            status,
            error,
        });
    }

    pub fn fail_step(&mut self, id: &str, label: &str, error: String) {
        self.error = Some(error.clone());
        self.push_step(id, label, DesktopApplyStepStatus::Failed, Some(error));
    }
}

impl Default for DesktopApplyResult {
    fn default() -> Self {
        Self::new()
    }
}
