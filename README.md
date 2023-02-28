# gRPC server in Rust
## Features

- Echo server
- Support command line arguments and environment variables, aka 12factor
- Prometheus metrics exporter
- Structured logger output

## Usage

```bash
$ baal --help
An Event stream filtering/mapping service for LeviMQ

Usage: baal [OPTIONS] --redis-cluster <REDIS_CLUSTER> --secret-keys <SECRET_KEYS>

Options:
  -f, --log-format <LOG_FORMAT>
          Log output format, default `plain`, accept: `json`, `pretty`, `plain`

          [env: LOG_FORMAT=]
          [default: plain]
          [possible values: plain, pretty, json]

      --grpc-listen <GRPC_ADDR>
          [env: GRPC_LISTEN_ADDR=]
          [default: 0.0.0.0:9090]

      --http-listen <HTTP_ADDR>
          [env: HTTP_LISTEN_ADDR=]
          [default: 0.0.0.0:8090]

      --http-health-path <HTTP_HEALTH_PATH>
          [env: HTTP_HEALTH_PATH=]
          [default: /health]

      --prometheus-prefix <PROMETHEUS_PREFIX>
          [env: PROMETHEUS_PREFIX=]
          [default: /metrics]

      --prometheus-enable <PROMETHEUS_ENABLE>
          [env: PROMETHEUS_ENABLE=]
          [default: true]

      --flatten-json
          Flatten fields in JSON log format, default is false

          [env: LOG_FLATTEN_JSON=]

      --without-ansi
          Enable log output ansi color in `pretty` or `plain` format, default is true

          [env: LOG_ANSI=]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## Environment Variables and Command Line Arguments

### HTTP Address

HTTP address to listen, for health check and Prometheus exporter.

- Env: `HTTP_LISTEN_ADDR`
- Arg: `--http-listen`
- Default: `0.0.0.0:8090`

### gRPC Address

HTTP address to listen, for health check and Prometheus exporter.

- Env: `HTTP_LISTEN_ADDR`
- Arg: `--http-listen`
- Default: `0.0.0.0:8090`

## Development

Requires Rust 1.64 or later.

Directories structure:

```bash
baal-rs
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
├── .cargo
│   └── config.toml
├── src
|── proto
└── target
```

## License

This project is licensed under the [MIT license](LICENSE).
