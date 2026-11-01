//! Machine-readable build contract for campaign launchers. Keep stdout JSON
//! only; a schema change must be explicit so old launchers fail closed.
use crate::base::{BitNum, Graph, MAX_SIZE};

pub fn json() -> String {
    format!(
        concat!(
            "{{\"schema\":\"graphy-capabilities-v1\",",
            "\"version\":\"{}\",\"bit_width\":{},\"max_order\":{},",
            "\"max_edges\":{},\"copy_count_bits\":128,",
            "\"nauty\":{},\"sat\":{},\"native\":{},",
            "\"orbital_enum_max_variables\":{}}}"
        ),
        env!("CARGO_PKG_VERSION"),
        BitNum::BITS,
        MAX_SIZE,
        Graph::triangle(MAX_SIZE),
        cfg!(feature = "nauty"),
        cfg!(feature = "sat"),
        cfg!(feature = "native"),
        crate::orbital_enum::MAX_VARIABLES,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_width_is_the_largest_supported_graph() {
        assert!(Graph::triangle(MAX_SIZE) <= BitNum::BITS as usize);
        assert!(Graph::triangle(MAX_SIZE + 1) > BitNum::BITS as usize);
        // Protect both the unsigned counts and signed deltas if MAX_SIZE changes.
        assert!(i128::try_from(crate::tools::factorial(MAX_SIZE).checked_mul(8).unwrap()).is_ok());
        assert!(json().contains("\"schema\":\"graphy-capabilities-v1\""));
    }
}
