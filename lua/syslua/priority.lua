---@class PriorityValue
---@field __value any
---@field __priority number
---@field __source {file: string, line: number}

---@class MergeableConfig
---@field __mergeable boolean
---@field separator? string

---@class PriorityModule
---@field PRIORITIES {FORCE: number, BEFORE: number, DEFAULT: number, AFTER: number}
---@field force fun(value: any): PriorityValue
---@field before fun(value: any): PriorityValue
---@field default fun(value: any): PriorityValue
---@field after fun(value: any): PriorityValue
---@field order fun(priority: number, value: any): PriorityValue
---@field mergeable fun(opts?: {separator?: string}): MergeableConfig
---@field merge fun(base: table, override: table): table
---@field wrap fun(value: any, priority: number, source?: {file: string, line: number}): PriorityValue
---@field unwrap fun(value: any): any
---@field is_priority fun(value: any): boolean
---@field is_mergeable fun(value: any): boolean
---@field get_priority fun(value: any): number
---@field get_source fun(level?: number): {file: string, line: number}

---@type PriorityModule
local M = {}

local PriorityMT = {
  __type = 'PriorityValue',
  __tostring = function(self)
    return string.format('PriorityValue(%s, priority=%d)', tostring(self.__value), self.__priority)
  end,
}

local MergeableMT = {
  __type = 'Mergeable',
}

local AccumulatedMT = {
  __type = 'Accumulated',
}

local MergedTableMT
MergedTableMT = {
  __type = 'MergedTable',
  __index = function(self, key)
    local raw = rawget(self, '__raw')
    local val = raw[key]
    if val ~= nil and type(val) == 'table' and getmetatable(val) == AccumulatedMT then
      return M._merge_values(val.__entries, val.__config)
    end
    return val
  end,
  __newindex = function(self, key, value)
    rawget(self, '__raw')[key] = value
  end,
  __pairs = function(self)
    local raw = rawget(self, '__raw')
    return function(t, k)
      local nk, nv = next(raw, k)
      if nv ~= nil and type(nv) == 'table' and getmetatable(nv) == AccumulatedMT then
        return nk, M._merge_values(nv.__entries, nv.__config)
      end
      return nk, nv
    end, raw, nil
  end,
}

M.PRIORITIES = {
  FORCE = 50,
  BEFORE = 500,
  DEFAULT = 1000,
  AFTER = 1500,
}

function M.get_source(level)
  if not debug or not debug.getinfo then
    return { file = 'unknown', line = 0 }
  end
  local info = debug.getinfo(level or 2, 'Sl')
  if info then
    local file = info.source or info.short_src or 'unknown'
    if file:sub(1, 1) == '@' then
      file = file:sub(2)
    end
    return {
      file = file,
      line = info.currentline or info.linedefined or 0,
    }
  end
  return { file = 'unknown', line = 0 }
end

function M.wrap(value, priority, source)
  return setmetatable({
    __value = value,
    __priority = priority,
    __source = source or M.get_source(3),
  }, PriorityMT)
end

function M.is_priority(value)
  return type(value) == 'table' and getmetatable(value) and getmetatable(value).__type == 'PriorityValue'
end

function M.unwrap(value)
  if M.is_priority(value) then
    return value.__value
  end
  return value
end

function M.get_priority(value)
  if M.is_priority(value) then
    return value.__priority
  end
  return M.PRIORITIES.DEFAULT
end

function M.force(value)
  return M.wrap(value, M.PRIORITIES.FORCE)
end

function M.before(value)
  return M.wrap(value, M.PRIORITIES.BEFORE)
end

function M.default(value)
  return M.wrap(value, M.PRIORITIES.DEFAULT)
end

function M.after(value)
  return M.wrap(value, M.PRIORITIES.AFTER)
end

function M.order(priority, value)
  if type(priority) ~= 'number' then
    error('priority.order: first argument must be a number', 2)
  end
  return M.wrap(value, priority)
end

function M.mergeable(opts)
  opts = opts or {}
  return setmetatable({
    __mergeable = true,
    separator = opts.separator,
  }, MergeableMT)
end

function M.is_mergeable(value)
  return type(value) == 'table' and getmetatable(value) and getmetatable(value).__type == 'Mergeable'
end

function M._is_accumulated(value)
  return type(value) == 'table' and getmetatable(value) and getmetatable(value).__type == 'Accumulated'
end

function M._make_accumulated(entries, config)
  return setmetatable({
    __entries = entries,
    __config = config,
  }, AccumulatedMT)
end

function M._make_merged_table(raw)
  return setmetatable({ __raw = raw }, MergedTableMT)
end

function M._unwrap_merged_table(t)
  if type(t) == 'table' and getmetatable(t) == MergedTableMT then
    return rawget(t, '__raw')
  end
  return t
end

function M._values_equal(a, b)
  if type(a) ~= type(b) then
    return false
  end
  if type(a) == 'table' then
    for k, v in pairs(a) do
      if b[k] ~= v then
        return false
      end
    end
    for k, v in pairs(b) do
      if a[k] ~= v then
        return false
      end
    end
    return true
  end
  return a == b
end

function M._priority_name(p)
  if p == M.PRIORITIES.FORCE then
    return 'force'
  elseif p == M.PRIORITIES.BEFORE then
    return 'before'
  elseif p == M.PRIORITIES.DEFAULT then
    return 'default'
  elseif p == M.PRIORITIES.AFTER then
    return 'after'
  else
    return 'custom'
  end
end

function M._format_value(v)
  if type(v) == 'string' then
    return string.format('%q', v)
  elseif type(v) == 'table' then
    return '{...}'
  else
    return tostring(v)
  end
end

function M._raise_conflict(key, entry1, entry2)
  local priority_name = M._priority_name(entry1.priority)

  local msg = string.format(
    [[
Priority conflict in '%s'

  Conflicting declarations at same priority level (%s: %d):

  File: %s:%d
    %s = %s

  File: %s:%d
    %s = %s

  Resolution options:
  1. Use priority.force() to explicitly override
  2. Use priority.before() or after() to adjust priority
  3. Use priority.order() for custom priority values
  4. Remove one of the conflicting declarations

  Built-in priorities:
    force:   50
    before:  500
    default: 1000
    after:   1500
]],
    key,
    priority_name,
    entry1.priority,
    entry1.source.file,
    entry1.source.line,
    key,
    M._format_value(entry1.value),
    entry2.source.file,
    entry2.source.line,
    key,
    M._format_value(entry2.value)
  )

  error(msg, 0)
end

function M._resolve_singular(key, entries)
  table.sort(entries, function(a, b)
    return a.priority < b.priority
  end)

  local winner = entries[1]
  for i = 2, #entries do
    if entries[i].priority == winner.priority then
      if not M._values_equal(entries[i].value, winner.value) then
        M._raise_conflict(key, winner, entries[i])
      end
    else
      break
    end
  end

  return winner.value
end

function M._merge_values(entries, config)
  table.sort(entries, function(a, b)
    return a.priority < b.priority
  end)

  if config.separator then
    local parts = {}
    for _, e in ipairs(entries) do
      table.insert(parts, tostring(e.value))
    end
    return table.concat(parts, config.separator)
  else
    local result = {}
    for _, e in ipairs(entries) do
      if type(e.value) == 'table' then
        for _, item in ipairs(e.value) do
          table.insert(result, item)
        end
      else
        table.insert(result, e.value)
      end
    end
    return result
  end
end

function M.merge(base, override)
  if base == nil then
    return override
  end
  if override == nil then
    return base
  end

  base = M._unwrap_merged_table(base)
  override = M._unwrap_merged_table(override)

  local result = {}
  local merge_configs = {}
  local accumulated = {}
  local all_values = {}

  for k, v in pairs(base) do
    if M.is_mergeable(v) then
      merge_configs[k] = v
    elseif M._is_accumulated(v) then
      merge_configs[k] = v.__config
      accumulated[k] = {}
      for _, entry in ipairs(v.__entries) do
        table.insert(accumulated[k], entry)
      end
    end
  end

  for k, v in pairs(base) do
    if not M.is_mergeable(v) and not M._is_accumulated(v) then
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'base', line = 0 },
      })
    end
  end

  for k, v in pairs(override) do
    if M.is_mergeable(v) then
      merge_configs[k] = v
    elseif M._is_accumulated(v) then
      merge_configs[k] = v.__config
      accumulated[k] = accumulated[k] or {}
      for _, entry in ipairs(v.__entries) do
        table.insert(accumulated[k], entry)
      end
    elseif not M.is_mergeable(v) then
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'override', line = 0 },
      })
    end
  end

  for k, entries in pairs(all_values) do
    if merge_configs[k] then
      accumulated[k] = accumulated[k] or {}
      for _, entry in ipairs(entries) do
        table.insert(accumulated[k], entry)
      end
    else
      result[k] = M._resolve_singular(k, entries)
    end
  end

  for k, entries in pairs(accumulated) do
    result[k] = M._make_accumulated(entries, merge_configs[k])
  end

  return M._make_merged_table(result)
end

return M
