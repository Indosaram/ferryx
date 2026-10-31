//! Background model turns through the same mhc RPC client as chat.
#[cfg(feature = "hub-slack")]
use super::protocol::Workspace;
use crate::error::{HubError, Result};
use async_trait::async_trait;

/// mhc RPC client for kibitzer, cron, judge, and compression turns.
pub struct RpcModel {
    binary: String,
    /// Overrides the whole-turn cap (cron turns use omo's tighter cron ceiling).
    total_timeout: Option<std::time::Duration>,
    #[cfg(feature = "hub-slack")]
    workspace: Workspace,
}

impl RpcModel {
    pub fn new(binary: impl Into<String>) -> Self {
        #[cfg(feature = "hub-slack")]
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .display()
            .to_string();
        Self {
            binary: binary.into(),
            total_timeout: None,
            #[cfg(feature = "hub-slack")]
            workspace: Workspace {
                roots: vec![cwd.clone()],
                cwd,
            },
        }
    }
}

impl RpcModel {
    /// Cron turns use a tighter whole-turn ceiling (default 600s).
    pub fn for_cron(binary: impl Into<String>) -> Self {
        let mut model = Self::new(binary);
        model.total_timeout = Some(cron_total_timeout(
            crate::hub_env::var("OMON_OMO_CRON_TURN_TOTAL_TIMEOUT_SECS")
                .ok()
                .as_deref(),
        ));
        model
    }

    pub fn total_timeout(&self) -> Option<std::time::Duration> {
        self.total_timeout
    }

    pub fn binary(&self) -> &str {
        &self.binary
    }
}

pub const CRON_TURN_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

pub fn cron_total_timeout(value: Option<&str>) -> std::time::Duration {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => CRON_TURN_TOTAL_TIMEOUT,
        Some(value) => match value.parse::<u64>().ok().filter(|secs| *secs > 0) {
            Some(secs) => std::time::Duration::from_secs(secs),
            None => {
                tracing::warn!(
                    value,
                    "invalid OMON_OMO_CRON_TURN_TOTAL_TIMEOUT_SECS, expected a positive integer"
                );
                CRON_TURN_TOTAL_TIMEOUT
            }
        },
    }
}

#[async_trait]
impl crate::lanes::kibitzer::ModelTurn for RpcModel {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.complete_with(prompt, None, &[]).await
    }

    async fn complete_with(
        &self,
        prompt: &str,
        model: Option<&str>,
        toolsets: &[String],
    ) -> Result<String> {
        if self.binary.trim().is_empty() {
            return Err(HubError::message("mhc binary is not configured"));
        }
        #[cfg(feature = "hub-slack")]
        {
            let mut config =
                super::omo_client::OmoClientConfig::new(self.binary.clone()).with_env_timeouts();
            config.workspace = Some(self.workspace.clone());
            if let Some(total) = self.total_timeout {
                config.total_timeout = total;
            }
            let request = super::omo_client::TurnRequest {
                thread_id: None,
                system_prompt: Some(prompt.to_owned()),
                // Every turn through this client is a background turn.
                model: Some(
                    model.map_or_else(crate::model_policy::background_model, str::to_owned),
                ),
                toolsets: (!toolsets.is_empty()).then(|| toolsets.to_vec()),
                text: prompt.to_owned(),
                pin_model: true,
                followups: None,
            };
            super::omo_client::run_turn(&config, &request, None)
                .await
                .map(|outcome| outcome.text)
        }
        #[cfg(not(feature = "hub-slack"))]
        {
            let _ = (prompt, model, toolsets);
            Err(HubError::message(
                "mhc RPC transport is compiled with hub-slack",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn rpc_model_runs_a_full_turn() {
        use crate::lanes::kibitzer::ModelTurn;
        use crate::meta::rpc_mock::{serve, Script};
        let decision_json = r#"{"wake":true,"reason":"r","suggested":"s","target_lane":null}"#;
        let (url, log) = serve(Script {
            messages: vec![decision_json.into()],
            approval: false,
            live: false,
        })
        .await;
        let model = RpcModel::new(url);
        let text = model.complete("state").await.unwrap();
        let decision = crate::lanes::kibitzer::parse_decision(&text).unwrap();
        assert!(decision.wake);
        let frames = log.frames.lock().unwrap().clone();
        let rpc: Vec<_> = frames
            .iter()
            .filter(|f| f["type"] != "fixture_launch")
            .collect();
        assert_eq!(rpc[0]["type"], "get_state");
        assert!(!frames.iter().any(|f| f["type"] == "open_session"));
        let launch = frames
            .iter()
            .find(|f| f["type"] == "fixture_launch")
            .unwrap();
        let args = launch["args"].as_array().unwrap();
        let flag = args.iter().position(|arg| arg == "--model").unwrap();
        assert_eq!(args[flag + 1], "mahoquot/gpt-6-luna");
        assert!(!frames.iter().any(|f| matches!(
            f["type"].as_str(),
            Some("set_model" | "set_thinking_level" | "set_auto_retry")
        )));
        assert!(frames.iter().any(|f| f["type"] == "prompt"));
    }

    #[test]
    fn the_cron_turn_ceiling_follows_omo() {
        assert_eq!(cron_total_timeout(None), CRON_TURN_TOTAL_TIMEOUT);
        assert_eq!(cron_total_timeout(Some(" ")), CRON_TURN_TOTAL_TIMEOUT);
        assert_eq!(
            cron_total_timeout(Some("120")),
            std::time::Duration::from_secs(120)
        );
        assert_eq!(cron_total_timeout(Some("0")), CRON_TURN_TOTAL_TIMEOUT);
        assert_eq!(cron_total_timeout(Some("later")), CRON_TURN_TOTAL_TIMEOUT);
    }
}
