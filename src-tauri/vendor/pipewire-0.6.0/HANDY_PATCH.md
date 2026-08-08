# Handy PipeWire patch

This is `pipewire` 0.6.0 from crates.io.

Ubuntu 22.04 provides PipeWire 0.3.48 headers. Newer Rust bindings do not
compile against these headers. The unmodified 0.6.0 binding also does not
destroy an owned `pw_stream` when `Stream` drops. Each Handy recording then
leaks a PipeWire data-loop thread and file descriptors.

The local change adds a conditional `Drop` implementation for the simple
streams that Handy creates through `pw_stream_new_simple`. It skips temporary
non-owning stream wrappers created for callbacks. Later `pipewire` releases
also separate owned streams from non-owning callback references.
