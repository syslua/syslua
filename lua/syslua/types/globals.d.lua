---@meta

---@alias Platform "x86_64-windows" | "aarch64-windows" | "x86_64-linux" | "aarch64-linux" | "i386-linux" | "x86_64-darwin" | "aarch64-darwin"
---@alias Os "windows" | "linux" | "darwin"
---@alias Arch "x86_64" | "aarch64" | "i386"

---@class Sys
---@field platform Platform Active platform
---@field os Os Operating system name
---@field arch Arch System architecture
---@field path PathHelpers File path utilities
---@field build fun(spec: BuildSpec): BuildRef Creates a build within the store
---@field bind fun(spec: BindSpec): BindRef Creates a binding to the active system

---@type Sys
---@diagnostic disable-next-line: missing-fields
sys = {}

---@type string
__dir = ''
