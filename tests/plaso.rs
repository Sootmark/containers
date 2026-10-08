//! plaso's Docker and CRI test files (Apache-2.0, `tests/fixtures/plaso/`):
//! every event plaso's `docker_container_config`, `docker_container_log`,
//! `docker_layer_config` and `cri_log` parsers read, read the same
//! (`tests/oracle/plaso.tsv`, written from plaso's output). plaso dates no
//! CRI line (its times read as zero) and names a container by its host
//! name; CRI lines are compared without their times, which are checked
//! apart.

use std::collections::BTreeSet;

use containers::{detect, read, Content, Kind};

const CONTAINER: &str =
    "docker/containers/e7d0b7ea5ccf08366e2b0c8afa2318674e8aefe802315378125d2bb83fe3110c";
const LAYER: &str =
    "docker/graph/3c9a9d7cc6a235eb2de58ca9ef3551c67ae42a991933ba4958d207b29142902b/json";

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/plaso/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn micros(time: Option<common::time::Ts>) -> String {
    (time.unwrap().ticks().unwrap() / 10).to_string()
}

fn escaped(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                format!("\\x{:02x}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn read_kind(path: &str, kind: Kind) -> Content {
    let data = fixture(path);
    assert_eq!(
        detect(&format!("var/lib/{path}"), &data),
        Some(kind),
        "{path}"
    );
    let read = read(kind, &data);
    assert_eq!(read.problems, Vec::<String>::new(), "{path}");
    read.content
}

#[test]
fn as_plaso_reads_them() {
    let mut got = BTreeSet::new();
    let Content::Container(container) =
        read_kind(&format!("{CONTAINER}/config.json"), Kind::DockerContainer)
    else {
        panic!("not a container");
    };
    let id = container.id.clone().unwrap();
    let host = container.hostname.clone().unwrap();
    got.insert(format!(
        "container\t{id}\t{host}\tCreation Time\t{}",
        micros(container.created)
    ));
    got.insert(format!(
        "container\t{id}\t{host}\tStart Time\t{}",
        micros(container.started)
    ));
    let Content::Log(lines) =
        read_kind(&format!("{CONTAINER}/container-json.log"), Kind::DockerLog)
    else {
        panic!("not a log");
    };
    for line in &lines {
        got.insert(format!(
            "log\t{}\t{}\t{}",
            line.stream,
            escaped(&line.text),
            micros(line.time)
        ));
    }
    let Content::Layer(layer) = read_kind(LAYER, Kind::DockerLayer) else {
        panic!("not a layer");
    };
    got.insert(format!(
        "layer\t{}\t{}",
        layer.command.unwrap(),
        micros(layer.created)
    ));
    let Content::Log(cri) = read_kind("cri.log", Kind::CriLog) else {
        panic!("not a log");
    };
    for line in &cri {
        let flag = if line.partial { "P" } else { "F" };
        got.insert(format!(
            "cri\t{}\t{flag}\t{}",
            line.stream,
            line.text.trim()
        ));
    }
    assert_eq!(
        cri[0].time.and_then(|t| t.to_iso8601()).as_deref(),
        Some("2016-10-06T00:17:09.6697942Z")
    );
    let expected: BTreeSet<&str> = include_str!("oracle/plaso.tsv").lines().collect();
    let got_refs: BTreeSet<&str> = got.iter().map(String::as_str).collect();
    assert_eq!(got_refs, expected);
    assert_eq!(container.name.as_deref(), Some("desperate_galileo"));
    assert_eq!(container.command.as_deref(), Some("/bin/bash"));
}
