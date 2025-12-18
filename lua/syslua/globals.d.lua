---@meta

---@class ExecOpts
---@field bin string Path to binary/executable to run (not a shell command string)
---@field args? string[] Optional: arguments to pass to the binary
---@field env? table<string,string> Optional: environment variables
---@field cwd? string Optional: working directory

---@class ActionCtx
---@field out string returns the store path
---@field fetch_url fun(self: ActionCtx, url: string, sha256: string): string Fetches a URL and returns the store path
---@field write_file fun(self: ActionCtx, path: string, content: string): string Writes content to a file in the store,
---returns the store path
---@field exec fun(self: ActionCtx, opts: string | ExecOpts, args?: string[]): string Performs a command during application, returns stdout

---@class BuildRef
---@field id string Build id
---@field inputs? table All inputs to the build
---@field outputs table All outputs from the build
---@field hash string Content-addressed hash

---@class BuildSpec
---@field id string Required: build id, must be unique
---@field inputs? table|fun(): table Optional: input data
---@field create fun(inputs: table, ctx: ActionCtx): table Required: build logic, returns outputs

---@class BindRef
---@field id string Binding id
---@field inputs? table All inputs to the binding
---@field outputs? table All outputs from the binding
---@field hash string Hash of actions for deduplication

---@class BindSpec
---@field id string Required: binding id, must be unique
---@field inputs? table|fun(): table Optional: input data
---@field create fun(inputs: table, ctx: ActionCtx): table | nil Required: binding logic, optionally returns outputs
---@field update? fun(outputs: table, inputs: table, ctx: ActionCtx): table | nil Optional: update logic, optionally returns outputs
---@field destroy fun(outputs: table, ctx: ActionCtx): nil Optional: cleanup logic, receives outputs from create or
---update

---@class PathHelpers
---@field resolve fun(...: string): string Resolves a sequence of path segments into an absolute path
---@field join fun(...: string): string Joins multiple path segments into a single path
---@field dirname fun(path: string): string Returns the directory name of the given path
---@field basename fun(path: string): string Returns the base name of the given path
---@field extname fun(path: string): string Returns the file extension of the given path
---@field is_absolute fun(path: string): boolean Checks if the given path is absolute
---@field normalize fun(path: string): string Normalizes the given path, resolving '..' and '.' segments
---@field relative fun(from: string, to: string): string Returns the relative path from one path to another
---@field split fun(path: string): table<string> Splits the path into its components

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
---@field register_ctx_method fun(name: string, fn: fun(ctx: ActionCtx, ...: any): any) Registers a custom method on ActionCtx
---@field unregister_ctx_method fun(name: string) Removes a registered custom method from ActionCtx

---@type Sys
---@diagnostic disable-next-line: missing-fields
sys = {}

---@type string
__dir = ''
