//! Any input gives content, problems or nothing, never a panic.

use containers::Kind;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_text(text in "[ -~\n{}\\[\\]\":,]{0,600}") {
        for kind in [Kind::DockerContainer, Kind::DockerLog, Kind::DockerLayer, Kind::CriLog] {
            let _ = containers::read(kind, text.as_bytes());
        }
        let _ = containers::detect("var/log/pods/x/0.log", text.as_bytes());
    }
}
