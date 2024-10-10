# `ndi-wayland-screenshare`
Shares a Wayland screen using NDI®.

## Crates
### `ndi-sys`
Low-level bindings to the NDI® SDK, including pre-built headers.

### `ndi`
High-level bindings to the NDI® SDK. This crate is a work in progress and currently only supports sending video frames.

The `ndi{,-sys}` crates does not provide its own copy of the NDI® SDK, but instead expects the SDK to be installed on the system or specified via the `NDI_RUNTIME_DIR_V5` environment variable.

### `ndi-wayland-screenshare`
Actual application that shares a Wayland screen using NDI®. Currently there is image tearing and the application is not very efficient.

---
NDI® is a registered trademark of NewTek, Inc.
http://ndi.tv/