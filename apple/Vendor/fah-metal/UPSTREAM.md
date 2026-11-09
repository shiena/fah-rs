# Upstream provenance

Source: https://github.com/LuckyIYI/fah-metal
Revision: 3ddc269a6a4ea8d921630ac4fc20d7d260375bca
License: Apache-2.0; shader portions adapt MLX under MIT (see NOTICE).

PoseNetwork.swift, Shaders/pose.metal, and tools/export.py are retained. Support.swift contains
minimal error and resource loading support from Types.swift and ImageWarp.swift.
The package excludes the upstream tracking pipeline, GUI, and MANO fitting.

Local change: PoseNetwork.resolve disables tensor ops for Simulator builds,
because MTLGPUFamily.metal4 is unavailable in the Simulator SDK. Device and macOS
backend selection remains unchanged. The shader source is unchanged.
