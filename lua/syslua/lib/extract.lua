local PATHS = {
  windows = {
    powershell = 'powershell.exe',
    tar = 'tar.exe',
  },
  darwin = {
    tar = '/usr/bin/tar',
    ditto = '/usr/bin/ditto',
  },
  linux = {
    tar = '/usr/bin/tar',
    unzip = '/usr/bin/unzip',
  },
}

---@alias ArchiveFormat "zip" | "tar.gz" | "tar.xz"

---@class ExtractOptions
---@field archive string Path to archive file (typically from lib.fetch_url)
---@field format ArchiveFormat Archive format
---@field strip_components? number Number of leading path components to strip

---@param opts ExtractOptions
---@return BuildRef
local function extract(opts)
  if not opts.archive then
    error("extract requires an 'archive' option")
  end
  if not opts.format then
    error("extract requires a 'format' option")
  end

  return sys.build({
    inputs = {
      archive = opts.archive,
      format = opts.format,
      strip_components = opts.strip_components or 0,
    },
    create = function(inputs, ctx)
      local paths = PATHS[sys.os]
      local archive = inputs.archive
      local format = inputs.format
      local strip = inputs.strip_components

      if format == 'zip' then
        if sys.os == 'windows' then
          ctx:exec({
            bin = paths.powershell,
            args = {
              '-NoProfile',
              '-Command',
              string.format('Expand-Archive -Path "%s" -DestinationPath "%s" -Force', archive, ctx.out),
            },
          })
        elseif sys.os == 'darwin' then
          ctx:exec({ bin = paths.ditto, args = { '-xk', archive, ctx.out } })
        else
          ctx:exec({ bin = paths.unzip, args = { '-q', archive, '-d', ctx.out } })
        end
      elseif format == 'tar.gz' then
        local args = { '-xzf', archive, '-C', ctx.out }
        if strip > 0 then
          table.insert(args, '--strip-components=' .. strip)
        end
        ctx:exec({ bin = paths.tar, args = args })
      elseif format == 'tar.xz' then
        local args = { '-xJf', archive, '-C', ctx.out }
        if strip > 0 then
          table.insert(args, '--strip-components=' .. strip)
        end
        ctx:exec({ bin = paths.tar, args = args })
      else
        error('Unsupported archive format: ' .. tostring(format))
      end

      return {
        out = ctx.out,
      }
    end,
  })
end

return extract
