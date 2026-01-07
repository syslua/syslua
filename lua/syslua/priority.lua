local M = {}

-- Forward declarations for private helper functions
local values_equal
local priority_name
local format_value
local raise_conflict
local resolve_singular
local merge_values
local unwrap_merged_table
local is_plain_table

local PriorityMT = {
  __type = 'PriorityValue',
  __tostring = function(self)
    return string.format('PriorityValue(%s, priority=%d)', tostring(self.__value), self.__priority)
  end,
}

local MergeableMT = {
  __type = 'Mergeable',
  __index = function(self, key)
    if key == 'separator' then
      return rawget(self, '__config').separator
    end
    return rawget(self, key)
  end,
}

local MergedTableMT = {
  __type = 'MergedTable',
  __index = function(self, key)
    local raw = rawget(self, '__raw')
    local val = raw[key]
    if M.is_mergeable(val) and #val.__entries > 0 then
      return merge_values(val.__entries, val.__config)
    end
    return val
  end,
  __newindex = function(self, key, value)
    rawget(self, '__raw')[key] = value
  end,
  __pairs = function(self)
    local raw = rawget(self, '__raw')
    return function(_, k)
      local nk, nv = next(raw, k)
      if nv and M.is_mergeable(nv) and #nv.__entries > 0 then
        return nk, merge_values(nv.__entries, nv.__config)
      end
      return nk, nv
    end,
      raw,
      nil
  end,
}

M.PRIORITIES = {
  FORCE = 50,
  BEFORE = 500,
  DEFAULT = 1000,
  AFTER = 1500,
}

-- Private helper function implementations

---@param a any
---@param b any
---@return boolean
values_equal = function(a, b)
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

---@param p number
---@return string
priority_name = function(p)
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

---@param v any
---@return string
format_value = function(v)
  if type(v) == 'string' then
    return string.format('%q', v)
  elseif type(v) == 'table' then
    return '{...}'
  else
    return tostring(v)
  end
end

---@param key string
---@param entry1 {value: any, priority: number, source: {file: string, line: number}}
---@param entry2 {value: any, priority: number, source: {file: string, line: number}}
raise_conflict = function(key, entry1, entry2)
  local pname = priority_name(entry1.priority)

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
    pname,
    entry1.priority,
    entry1.source.file,
    entry1.source.line,
    key,
    format_value(entry1.value),
    entry2.source.file,
    entry2.source.line,
    key,
    format_value(entry2.value)
  )

  error(msg, 0)
end

---@param key string
---@param entries {value: any, priority: number, source: {file: string, line: number}, explicit: boolean}[]
---@return any
resolve_singular = function(key, entries)
  table.sort(entries, function(a, b)
    if a.priority ~= b.priority then
      return a.priority < b.priority
    end
    if a.explicit and not b.explicit then
      return true
    end
    if b.explicit and not a.explicit then
      return false
    end
    if a.source.file == 'override' and b.source.file ~= 'override' then
      return true
    end
    if b.source.file == 'override' and a.source.file ~= 'override' then
      return false
    end
    return false
  end)

  local winner = entries[1]
  for i = 2, #entries do
    if entries[i].priority == winner.priority and entries[i].explicit and winner.explicit then
      if not values_equal(entries[i].value, winner.value) then
        raise_conflict(key, winner, entries[i])
      end
    elseif entries[i].priority ~= winner.priority then
      break
    end
  end

  return winner.value
end

---@param entries {value: any, priority: number}[]
---@param config {separator?: string}
---@return any
merge_values = function(entries, config)
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

---@param t any
---@return table
unwrap_merged_table = function(t)
  if type(t) == 'table' and getmetatable(t) == MergedTableMT then
    return rawget(t, '__raw')
  end
  return t
end

---@param t any
---@return boolean
is_plain_table = function(t)
  if type(t) ~= 'table' or getmetatable(t) then
    return false
  end
  local n = 0
  for k in pairs(t) do
    if type(k) ~= 'number' or k < 1 or k ~= math.floor(k) then
      return true
    end
    n = n + 1
  end
  return n ~= #t
end

-- Public API

---@param level? number
---@return {file: string, line: number}
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

---@param value any
---@param priority number
---@param source? {file: string, line: number}
function M.wrap(value, priority, source)
  return setmetatable({
    __value = value,
    __priority = priority,
    __source = source or M.get_source(3),
  }, PriorityMT)
end

---@param value any
---@return boolean
function M.is_priority(value)
  return type(value) == 'table' and getmetatable(value) == PriorityMT
end

---@param value any
---@return any
function M.unwrap(value)
  if M.is_priority(value) then
    return value.__value
  end
  return value
end

---@param value any
---@return number
function M.get_priority(value)
  if M.is_priority(value) then
    return value.__priority
  end
  return M.PRIORITIES.DEFAULT
end

---@param value any
function M.force(value)
  return M.wrap(value, M.PRIORITIES.FORCE)
end

---@param value any
function M.before(value)
  return M.wrap(value, M.PRIORITIES.BEFORE)
end

---@param value any
function M.default(value)
  return M.wrap(value, M.PRIORITIES.DEFAULT)
end

---@param value any
function M.after(value)
  return M.wrap(value, M.PRIORITIES.AFTER)
end

---@param priority number
---@param value any
function M.order(priority, value)
  if type(priority) ~= 'number' then
    error('priority.order: first argument must be a number', 2)
  end
  return M.wrap(value, priority)
end

---@param opts? {separator?: string}
function M.mergeable(opts)
  opts = opts or {}
  return setmetatable({
    __config = { separator = opts.separator },
    __entries = {},
  }, MergeableMT)
end

---@param value any
---@return boolean
function M.is_mergeable(value)
  return type(value) == 'table' and getmetatable(value) == MergeableMT
end

---@param base? table
---@param override? table
---@param _path? string
---@return table?
function M.merge(base, override, _path)
  _path = _path or ''

  if base == nil then
    return override
  end
  if override == nil then
    return base
  end

  base = unwrap_merged_table(base)
  override = unwrap_merged_table(override)

  local result = {}
  local mergeables = {}
  local all_values = {}
  local nested = {}

  local function key_path(k)
    return _path == '' and tostring(k) or (_path .. '.' .. tostring(k))
  end

  for k, v in pairs(base) do
    local unwrapped_v = unwrap_merged_table(v)
    if M.is_mergeable(v) then
      mergeables[k] = setmetatable({
        __config = v.__config,
        __entries = {},
      }, MergeableMT)
      for _, entry in ipairs(v.__entries) do
        table.insert(mergeables[k].__entries, entry)
      end
    elseif is_plain_table(unwrapped_v) or (v ~= unwrapped_v) then
      nested[k] = { base = unwrapped_v }
    else
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'base', line = 0 },
        explicit = M.is_priority(v),
      })
    end
  end

  for k, v in pairs(override) do
    if M.is_mergeable(v) then
      if not mergeables[k] then
        mergeables[k] = setmetatable({
          __config = v.__config,
          __entries = {},
        }, MergeableMT)
      end
      for _, entry in ipairs(v.__entries) do
        table.insert(mergeables[k].__entries, entry)
      end
    elseif is_plain_table(v) and nested[k] then
      nested[k].override = v
    elseif is_plain_table(v) then
      local base_v = base[k]
      local unwrapped_base = unwrap_merged_table(base_v)
      if is_plain_table(unwrapped_base) or (base_v ~= unwrapped_base) then
        nested[k] = { base = unwrapped_base, override = v }
      end
    else
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'override', line = 0 },
        explicit = M.is_priority(v),
      })
    end
  end

  for k, entries in pairs(all_values) do
    if mergeables[k] then
      for _, entry in ipairs(entries) do
        table.insert(mergeables[k].__entries, entry)
      end
    else
      result[k] = resolve_singular(key_path(k), entries)
    end
  end

  for k, tables in pairs(nested) do
    if tables.override then
      result[k] = M.merge(tables.base, tables.override, key_path(k))
    else
      result[k] = tables.base
    end
  end

  for k, mergeable in pairs(mergeables) do
    result[k] = mergeable
  end

  return setmetatable({ __raw = result }, MergedTableMT)
end

return M
