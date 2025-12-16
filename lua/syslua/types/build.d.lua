---@meta

---@class BuildRef
---@field name string Build name
---@field version? string Version string
---@field inputs? table All inputs to the build
---@field outputs table All outputs from the build
---@field hash string Content-addressed hash

---@class BuildCmdOptions
---@field cmd string Command to execute
---@field env? table<string,string> Optional: environment variables
---@field cwd? string Optional: working directory

---@class BuildCtx
---@field out string returns the store path
---@field fetch_url fun(self: BuildCtx, url: string, sha256: string): string Fetches a URL and verifies its SHA256 checksum
---@field cmd fun(self: BuildCtx, opts: string | BuildCmdOptions): string Executes a command during the build, returns output

---@class BuildSpec
---@field name string Required: build name
---@field version? string Optional: version string
---@field inputs? table|fun(): table Optional: input data
---@field apply fun(inputs: table, ctx: BuildCtx): table Required: build logic, returns outputs
