# containers

Container forensics from the host's files: what ran in Docker and Kubernetes containers, as whom, with which mounts and ports, and what it printed. One dependency, its sibling `sootmark-common` (JSON, times).

```toml
[dependencies]
sootmark-containers = "0.1"
```

```rust
let path = "var/lib/docker/containers/e7d0…/config.v2.json";
let data = std::fs::read(path)?;
if let Some(kind) = containers::detect(path, &data) {
    println!("{:?}", containers::read(kind, &data).content);
}
```

## What you get

- Docker container configurations (`config.v2.json`, `config.json`): id, name, host name, image, command, user, environment, mounts, published ports, privileged, created, started, finished, running.
- Docker `json-file` logs (`<id>-json.log`): each line written, its stream and time.
- Docker image layer configurations (`graph/<id>/json`, `imagedb/content/…`): when each layer was made, the command that made it, its author.
- CRI container logs (Kubernetes, containerd, CRI-O): each line's time, stream, partial or full, and text.
- Times in UTC. Damage goes to `problems`, never a panic.

## How it's checked

- plaso's Docker and CRI test files (Apache-2.0, `tests/fixtures/plaso/`): every event its Docker and CRI parsers read, read the same (`tests/oracle/`); CRI times, which plaso doesn't read, checked apart.
- Property tests: arbitrary text never panics.

## Licence

MIT or Apache-2.0, at your option.
