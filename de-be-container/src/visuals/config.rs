//! Modular configuration for Data Visuals & Semantic Relationship Topology (Specs 13 & 14).

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error, PartialEq)]
pub enum VisualsConfigError {
    #[error("visuals.max_chart_points must be between 10 and 100000, got {0}")]
    InvalidMaxChartPoints(usize),
    #[error("visuals.max_topology_nodes must be between 10 and 5000, got {0}")]
    InvalidMaxTopologyNodes(usize),
    #[error("visuals.max_topology_edges must be between 10 and 20000, got {0}")]
    InvalidMaxTopologyEdges(usize),
    #[error("visuals.default_ego_hops must be between 1 and 5, got {0}")]
    InvalidDefaultEgoHops(u8),
    #[error("visuals.graphrag_base_url must use http:// or https:// scheme when set")]
    InvalidGraphragBaseUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualsConfig {
    pub max_chart_points: usize,
    pub max_topology_nodes: usize,
    pub max_topology_edges: usize,
    pub default_ego_hops: u8,
    pub graphrag_base_url: Option<Url>,
}

impl Default for VisualsConfig {
    fn default() -> Self {
        Self {
            max_chart_points: 5000,
            max_topology_nodes: 250,
            max_topology_edges: 500,
            default_ego_hops: 2,
            graphrag_base_url: None,
        }
    }
}

impl VisualsConfig {
    pub fn validate(&self) -> Result<(), VisualsConfigError> {
        if self.max_chart_points < 10 || self.max_chart_points > 100_000 {
            return Err(VisualsConfigError::InvalidMaxChartPoints(
                self.max_chart_points,
            ));
        }
        if self.max_topology_nodes < 10 || self.max_topology_nodes > 5_000 {
            return Err(VisualsConfigError::InvalidMaxTopologyNodes(
                self.max_topology_nodes,
            ));
        }
        if self.max_topology_edges < 10 || self.max_topology_edges > 20_000 {
            return Err(VisualsConfigError::InvalidMaxTopologyEdges(
                self.max_topology_edges,
            ));
        }
        if self.default_ego_hops == 0 || self.default_ego_hops > 5 {
            return Err(VisualsConfigError::InvalidDefaultEgoHops(
                self.default_ego_hops,
            ));
        }
        if let Some(ref url) = self.graphrag_base_url {
            let scheme = url.scheme();
            if scheme != "http" && scheme != "https" {
                return Err(VisualsConfigError::InvalidGraphragBaseUrl);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visuals_config_validation_rejects_out_of_range_bounds() {
        assert!(VisualsConfig::default().validate().is_ok());

        let with_http_url = VisualsConfig {
            graphrag_base_url: Some(Url::parse("http://graphrag:8080/api/graphrag/v1").unwrap()),
            ..Default::default()
        };
        assert!(with_http_url.validate().is_ok());

        let too_few_points = VisualsConfig {
            max_chart_points: 5,
            ..Default::default()
        };
        assert_eq!(
            too_few_points.validate(),
            Err(VisualsConfigError::InvalidMaxChartPoints(5))
        );

        let too_few_nodes = VisualsConfig {
            max_topology_nodes: 3,
            ..Default::default()
        };
        assert_eq!(
            too_few_nodes.validate(),
            Err(VisualsConfigError::InvalidMaxTopologyNodes(3))
        );

        let zero_hops = VisualsConfig {
            default_ego_hops: 0,
            ..Default::default()
        };
        assert_eq!(
            zero_hops.validate(),
            Err(VisualsConfigError::InvalidDefaultEgoHops(0))
        );

        let excessive_hops = VisualsConfig {
            default_ego_hops: 6,
            ..Default::default()
        };
        assert_eq!(
            excessive_hops.validate(),
            Err(VisualsConfigError::InvalidDefaultEgoHops(6))
        );

        let bad_url = VisualsConfig {
            graphrag_base_url: Some(Url::parse("ftp://graphrag:8080").unwrap()),
            ..Default::default()
        };
        assert_eq!(
            bad_url.validate(),
            Err(VisualsConfigError::InvalidGraphragBaseUrl)
        );
    }
}
