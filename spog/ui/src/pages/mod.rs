//! Pages in the console

mod advisory;
mod advisory_search;
mod chicken;
mod cve;
mod index;
mod not_found;
mod not_logged_in;
mod sbom;
mod sbom_search;
mod scanner;
mod search;

pub use advisory::*;
pub use advisory_search::*;
pub use chicken::*;
pub use cve::*;
pub use index::*;
pub use not_found::*;
pub use not_logged_in::*;
pub use sbom::*;
pub use sbom_search::Package;
pub use scanner::*;
pub use search::Search;
