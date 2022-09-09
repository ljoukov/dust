use crate::app::BlockExecution;
use crate::blocks::block::BlockType;
use crate::utils;
use anyhow::{anyhow, Result};
use async_fs::File;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub struct RunConfig {
    pub start_time: u64,
    pub app_hash: String,
    pub blocks: HashMap<String, Value>,
}

impl RunConfig {
    pub fn config_for_block(&self, name: &str) -> Option<&Value> {
        self.blocks.get(name)
    }

    pub async fn load(run_id: &str) -> Result<Self> {
        let root_path = utils::init_check().await?;
        let runs_dir = root_path.join(".runs");

        assert!(runs_dir.is_dir().await);
        let run_dir = runs_dir.join(run_id);

        if !run_dir.exists().await {
            Err(anyhow!("Run `{}` does not exist", run_id))?;
        }

        let config_path = run_dir.join("config.json");

        let config_data = async_std::fs::read_to_string(config_path).await?;
        let config: RunConfig = serde_json::from_str(&config_data)?;

        Ok(config)
    }
}

/// Execution represents the full execution of an app on input data.
#[derive(PartialEq)]
pub struct Run {
    run_id: String,
    config: RunConfig,
    // List of blocks (in order with name) and their execution.
    // The outer vector represents blocks
    // The inner-outer vector represents inputs
    // The inner-inner vector represents mapped outputs
    // If execution was interrupted by errors, the non-executed block won't be present. If a block
    // on a particular Env was not executed due to a conditional execution, its BlockExecution will
    // be present but both output and error will be None.
    // TODO(spolu): note that there is a lot of repetition here in particular through the env
    // variables, will need to be revisited but that's a fair enough starting point.
    pub traces: Vec<((BlockType, String), Vec<Vec<BlockExecution>>)>,
}

impl Run {
    pub fn new(config: RunConfig) -> Self {
        Self {
            run_id: utils::new_id(),
            config,
            traces: vec![],
        }
    }

    pub fn config(&self) -> &RunConfig {
        &self.config
    }

    pub async fn store(&self) -> Result<()> {
        let root_path = utils::init_check().await?;
        let runs_dir = root_path.join(".runs");

        assert!(runs_dir.is_dir().await);
        let run_dir = runs_dir.join(&self.run_id);
        assert!(!run_dir.exists().await);

        utils::action(&format!("Creating directory {}", run_dir.display()));
        async_std::fs::create_dir_all(&run_dir).await?;

        let config_path = run_dir.join("config.json");
        utils::action(&format!("Writing run config in {}", config_path.display()));
        {
            let mut file = File::create(config_path).await?;
            file.write_all(serde_json::to_string(&self.config)?.as_bytes())
                .await?;
            file.flush().await?;
        }

        for (block_idx, ((block_type, name), block_execution)) in self.traces.iter().enumerate() {
            let block_dir =
                run_dir.join(format!("{}-{}_{}", block_idx, block_type.to_string(), name));
            utils::action(&format!("Creating directory {}", block_dir.display()));
            async_std::fs::create_dir_all(&block_dir).await?;
            for (input_idx, executions) in block_execution.iter().enumerate() {
                let executions_path = block_dir.join(format!("{}.json", input_idx));
                {
                    let mut file = File::create(executions_path).await?;
                    file.write_all(serde_json::to_string(executions)?.as_bytes())
                        .await?;
                    file.flush().await?;
                }
            }
        }
        utils::done(&format!(
            "Run `{}` for app version `{}` stored",
            self.run_id, self.config.app_hash
        ));

        Ok(())
    }

    pub async fn load(run_id: &str) -> Result<Self> {
        let config = RunConfig::load(run_id).await?;

        Ok(Run {
            run_id: run_id.to_string(),
            config,
            traces: vec![],
        })
    }
}

pub async fn cmd_inspect(run_id: &str, block: &str) -> Result<()> {
    let run = Run::load(run_id).await?;

    Ok(())
}

pub async fn cmd_list() -> Result<()> {
    let root_path = utils::init_check().await?;
    let runs_dir = root_path.join(".runs");

    let mut entries = async_std::fs::read_dir(runs_dir).await?;

    let mut runs: Vec<(String, RunConfig)> = vec![];
    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir().await {
            let run_id = path.file_name().unwrap().to_str().unwrap();
            let config = RunConfig::load(run_id).await?;
            runs.push((run_id.to_string(), config));
        }
    }

    runs.sort_by(|a, b| b.1.start_time.cmp(&a.1.start_time));

    runs.iter().for_each(|(run_id, config)| {
        utils::info(&format!(
            "Run: {} app_hash={} start_time={}",
            run_id,
            config.app_hash,
            utils::utc_date_from(config.start_time),
        ));
    });
    Ok(())
}