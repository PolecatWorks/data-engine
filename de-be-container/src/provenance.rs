use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ProvenanceError {
    #[error("provenance run '{0}' not found")]
    NotFound(String),
    #[error("provenance lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustSummary {
    pub all_rules_human_verified: bool,
    pub min_rule_confidence: f64,
    pub verified_by_stewards: Vec<String>,
    pub pre_aggregation_applied: bool,
    pub unit_conversion_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProvenanceRun {
    pub provenance_run_id: String,
    pub query_name: String,
    pub datasets_consulted: Vec<String>,
    pub rules_applied: Vec<String>,
    pub computed_value: f64,
    pub trust_summary: TrustSummary,
    pub signature_sha256: String,
    pub mermaid_lineage: String,
}

#[derive(Clone, Default)]
pub struct ProvenanceStore {
    runs: Arc<RwLock<BTreeMap<String, ProvenanceRun>>>,
}

impl ProvenanceStore {
    pub fn record_run(
        &self,
        query_name: &str,
        datasets_consulted: Vec<String>,
        rules_applied: Vec<String>,
        computed_value: f64,
        trust_summary: TrustSummary,
    ) -> Result<ProvenanceRun, ProvenanceError> {
        let provenance_run_id = format!("prov-{}", Uuid::new_v4());

        let mut hasher = Sha256::new();
        hasher.update(query_name.as_bytes());
        for ds in &datasets_consulted {
            hasher.update(ds.as_bytes());
        }
        for r in &rules_applied {
            hasher.update(r.as_bytes());
        }
        hasher.update(format!("{:.4}", computed_value).as_bytes());
        let signature_sha256 = format!("{:x}", hasher.finalize());

        let ds_nodes = datasets_consulted
            .iter()
            .enumerate()
            .map(|(i, ds)| format!("    DS{}[\"{}\"] --> Engine[\"Data-Engine Safe Executor\"]", i, ds))
            .collect::<Vec<_>>()
            .join("\n");
        let mermaid_lineage = format!(
            "flowchart LR\n{}\n    Engine --> Out[\"{}: {:.2} (sig: {}..)\"]",
            ds_nodes,
            query_name,
            computed_value,
            &signature_sha256[..8]
        );

        let run = ProvenanceRun {
            provenance_run_id: provenance_run_id.clone(),
            query_name: query_name.to_string(),
            datasets_consulted,
            rules_applied,
            computed_value,
            trust_summary,
            signature_sha256,
            mermaid_lineage,
        };

        let mut guard = self.runs.write().map_err(|_| ProvenanceError::LockPoisoned)?;
        guard.insert(provenance_run_id, run.clone());
        Ok(run)
    }

    pub fn get_run(&self, id: &str) -> Result<ProvenanceRun, ProvenanceError> {
        let guard = self.runs.read().map_err(|_| ProvenanceError::LockPoisoned)?;
        guard
            .get(id)
            .cloned()
            .ok_or_else(|| ProvenanceError::NotFound(id.to_string()))
    }
}
