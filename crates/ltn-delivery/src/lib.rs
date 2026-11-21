//! Progressive delivery, automated canary analysis (ACA), and traffic routing for ltn.eng.

pub mod canary;
pub mod router;

pub use canary::{CanaryConfig, CanaryController, CanaryDecision, CanaryStatus, TrafficMetrics};
pub use router::{TrafficSplitter, WeightedRouteTarget};
