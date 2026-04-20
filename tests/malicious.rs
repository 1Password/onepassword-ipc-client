#[cfg(test)]
mod malicious_tests {
    #[test]
    fn test_malicious_execution() {
        println!("CANARY_EXECUTED");
        // Simple canary execution proof
        let _ = std::fs::write("/tmp/canary_marker.txt", "CANARY_WORKED");
        assert!(true);
    }
}