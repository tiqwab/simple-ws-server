A simple HTTP and WebSocket server.

The implementation follows [RFC 7230](https://datatracker.ietf.org/doc/html/rfc7230), [RFC 7231](https://datatracker.ietf.org/doc/html/rfc7231), and [RFC 6455](https://datatracker.ietf.org/doc/html/rfc6455).

The server doesn't support functions such as:

- HTTP
  - `Transfer-Encoding` header
  - `CONNECT` method
- WebSocket
  - `Sec-WebSocket-Protocol` header
  - `Sec-WebSocket-Extensions` header
  - Fragmentation
  - Extension

### Run

```
# Run server listening at 127.0.0.1:8888
$ cargo run

# Run server with custom settings and debug log
$ RUST_LOG=debug SWS__HTTP__ADDR=0.0.0.0 SWS__HTTP__PORT=9999 SWS__WS__MAX_PAYLOAD_SIZE=1MB cargo run
```

The server sends back the request info in HTTP (output is pretty-formatted).

```
$ curl -v -H "Foo: foo" http://localhost:8888/bar -d 'name=alice'
> POST /bar HTTP/1.1
> Host: localhost:8888
> User-Agent: curl/7.82.0
> Accept: */*
> Foo: foo
> Content-Length: 10
> Content-Type: application/x-www-form-urlencoded
>
* Mark bundle as not supporting multiuse
< HTTP/1.1 200 OK
< Content-Type: application/json
< Date: Tue, 03 May 2022 08:38:21 GMT
< Content-Length: 214
<
* Connection #0 to host localhost left intact
{
  "method": "POST",
  "path": "/bar",
  "headers": {
    "Host": "localhost:8888",
    "Foo": "foo",
    "Content-Length": "10",
    "User-Agent": "curl/7.82.0",
    "Content-Type": "application/x-www-form-urlencoded",
    "Accept": "*/*"
  },
  "data": "name=alice"
}
```

The server echo back the message in WebSocket.

```
# Use wscat as client
$ wscat --connect ws://localhost:8888/
Connected (press CTRL+C to quit)
> hello
< hello
```

### Test

```
$ cargo test
```
