### Run

```
# Run server listening at 127.0.0.1:8888
$ cargo run

# Run server with custom settings and debug log
$ RUST_LOG=debug SWS__HTTP__ADDR=0.0.0.0 SWS__HTTP__PORT=9999 SWS__WS__MAX_PAYLOAD_SIZE=1MB cargo run
```

### Test

```
$ cargo test
```
