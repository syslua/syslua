---@meta

---@class BindRef
---@field inputs? table All inputs to the binding
---@field outputs? table All outputs from the binding
---@field hash string Hash of actions for deduplication

---@class BindCmdOptions
---@field cmd string Command to execute
---@field env? table<string,string> Optional: environment variables
---@field cwd? string Optional: working directory

---@class BindCtx
---@field out string returns the store path
---@field cmd fun(self: BindCtx, opts: string | BindCmdOptions): string Performs a command during application, returns outputs

---@class BindSpec
---@field inputs? table|fun(): table Optional: input data
---@field apply fun(inputs: table, ctx: BindCtx): table | nil Required: binding logic, optionally returns outputs
---@field destroy? fun(outputs: table, ctx: BindCtx): nil Optional: cleanup logic, receives outputs from apply
