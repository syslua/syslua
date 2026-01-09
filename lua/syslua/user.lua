---@class syslua.user
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.user.Options
---@field description? string User description/comment
---@field homeDir string Home directory path (required)
---@field config string Path to user's syslua config (required)
---@field shell? BuildRef Login shell package
---@field initialPassword? string Initial password (plaintext, set on creation only)
---@field groups? string[] Groups to add user to (must exist)
---@field preserveHomeOnRemove? boolean Keep home directory when user is removed (default: false)

---@alias syslua.user.UserMap table<string, syslua.user.Options>

-- ============================================================================
-- Constants
-- ============================================================================

local BIND_ID_PREFIX = '__syslua_user_'

-- ============================================================================
-- Validation
-- ============================================================================

---@param name string
---@param opts syslua.user.Options
local function validate_user_options(name, opts)
  if not opts.homeDir then
    error(string.format("user '%s': homeDir is required", name), 0)
  end
  if not opts.config then
    error(string.format("user '%s': config is required", name), 0)
  end
  if not sys.is_elevated then
    error('syslua.user requires elevated privileges (root/Administrator)', 0)
  end
end

-- ============================================================================
-- Public API
-- ============================================================================

---Set up users according to the provided definitions
---@param users syslua.user.UserMap
function M.setup(users)
  if not users or next(users) == nil then
    error('syslua.user.setup: at least one user definition is required', 2)
  end

  for name, opts in pairs(users) do
    validate_user_options(name, opts)
    -- TODO: Create bind for user
  end
end

return M
