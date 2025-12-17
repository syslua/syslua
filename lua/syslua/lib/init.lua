---@class syslua.lib
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.lib.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.lib." .. k .. "' not found")
    end
  end,
})

---@class FetchUrlOptions
---@field url string
---@field sha256 string

---Fetches a file from a URL and verifies its SHA256 checksum.
---@param opts FetchUrlOptions
---@return BuildRef
function M.fetch_url(opts)
  if not opts.url then
    error("fetch_url requires a 'url' option")
  end
  if not opts.sha256 then
    error("fetch_url requires a 'sha256' option")
  end

  local basename = sys.path.basename(opts.url)

  return sys.build({
    name = 'fetch-' .. basename,
    inputs = {
      url = opts.url,
      sha256 = opts.sha256,
    },
    apply = function(inputs, ctx)
      local result = ctx:fetch_url(inputs.url, inputs.sha256)
      return {
        out = result,
      }
    end,
  })
end

return M
