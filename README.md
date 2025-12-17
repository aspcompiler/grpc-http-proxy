This repo contains several http-proxy prototypes capable of routing gRPC calls. These are prototypes. Please do not use them in production.

* hyper-edition: This directory contains a proxy built over the low level hyper crate. It has 2000+ lines of functional code.
* pigora-edition: This directory contains a proxy built over the high level pinggora routing library. It only needs 50-100
lines of functional code to configure the library.

Here some features for both editions:

1. Optional TLS, mTLS support for the proxy server. The proxy only communicates with the backend in clear.
2. Support TLS ALPN since this is expected by some gRPC clients. Otherwise it will not use http/2 protocol.
3. Support for gRPC bidirectional streaming by streaming the http trailers, i.e., headers after a part of body.
