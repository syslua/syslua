-- MIT License
--
-- Copyright (c) 2023 Andrey Listopadov
--
-- Permission is hereby granted, free of charge, to any person obtaining a copy
-- of this software and associated documentation files (the "Software"), to deal
-- in the Software without restriction, including without limitation the rights
-- to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
-- copies of the Software, and to permit persons to whom the Software is
-- furnished to do so, subject to the following conditions:
--
-- The above copyright notice and this permission notice shall be included in all
-- copies or substantial portions of the Software.
--
-- THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
-- IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
-- FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
-- AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
-- LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
-- OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
-- SOFTWARE.

--- String interpolation module (Lua 5.4+)
--- Uses {{}} delimiters to avoid confusion with shell ${} syntax
--- Usage: interpolate("Hello {{name}}!", {name = "world"})
--- Or with locals: local name = "world"; interpolate("Hello {{name}}!")

local function get_locals()
  local variables = {}
  if not debug or not debug.getlocal then
    return variables
  end
  local idx = 1
  while true do
    local name, value = debug.getlocal(3, idx)
    if not name then
      break
    end
    variables[name] = value
    idx = idx + 1
  end
  return variables
end

local function get_upvalues()
  local variables = {}
  if not debug or not debug.getinfo or not debug.getupvalue then
    return variables
  end
  local func = debug.getinfo(3, 'f').func
  local idx = 1
  while true do
    local name, value = debug.getupvalue(func, idx)
    if not name then
      break
    end
    variables[name] = value
    idx = idx + 1
  end
  return variables
end

local function eval(code, env)
  local chunk = assert(load('return ' .. code, nil, 't', env))
  return chunk()
end

--- Simple cursor-based string reader
local function create_cursor(str)
  return { str = str, pos = 1, len = #str }
end

local function peek(cursor, n)
  n = n or 1
  if cursor.pos > cursor.len then
    return nil
  end
  return cursor.str:sub(cursor.pos, cursor.pos + n - 1)
end

local function advance(cursor, n)
  n = n or 1
  local result = peek(cursor, n)
  cursor.pos = cursor.pos + n
  return result
end

local function at_end(cursor)
  return cursor.pos > cursor.len
end

--- Parse a format specifier after :%
local function parse_format(cursor)
  local format = {}
  while not at_end(cursor) do
    local two_chars = peek(cursor, 2)
    if two_chars == '}}' then
      advance(cursor, 2)
      return table.concat(format)
    end
    local char = peek(cursor)
    if char and char:match('%s') then
      error(string.format('invalid format specifier: %q', table.concat(format) .. char))
    else
      format[#format + 1] = advance(cursor)
    end
  end
  error("unmatched '{{'", 2)
end

--- Skip over a string literal (handles escapes)
local function skip_string(cursor, delim)
  local content = { delim }
  while not at_end(cursor) do
    local char = advance(cursor)
    content[#content + 1] = char
    if char == '\\' and not at_end(cursor) then
      content[#content + 1] = advance(cursor)
    elseif char == delim then
      return table.concat(content)
    end
  end
  error('unmatched ' .. delim, 2)
end

--- Skip over nested braces (for table literals inside expressions)
local function skip_braces(cursor)
  local content = {}
  local nesting = 1
  while not at_end(cursor) and nesting > 0 do
    local char = peek(cursor)
    if char == '{' then
      nesting = nesting + 1
      content[#content + 1] = advance(cursor)
    elseif char == '}' then
      nesting = nesting - 1
      if nesting > 0 then
        content[#content + 1] = advance(cursor)
      end
    elseif char == '"' or char == "'" then
      advance(cursor)
      content[#content + 1] = skip_string(cursor, char)
    else
      content[#content + 1] = advance(cursor)
    end
  end
  if nesting > 0 then
    error("unmatched '{'", 2)
  end
  return table.concat(content)
end

--- Parse an interpolation expression {{expr}} or {{expr=}} or {{expr:%fmt}}
local function parse_expression(cursor)
  local expr = {}

  while not at_end(cursor) do
    local two_chars = peek(cursor, 2)

    if two_chars == '}}' then
      advance(cursor, 2)
      return table.concat(expr), '%s', tostring
    elseif peek(cursor) == '{' then
      advance(cursor)
      expr[#expr + 1] = '{' .. skip_braces(cursor) .. '}'
    elseif peek(cursor) == '"' or peek(cursor) == "'" then
      local delim = advance(cursor)
      expr[#expr + 1] = skip_string(cursor, delim)
    elseif peek(cursor) == '=' then
      -- Check for debug expression syntax: {{expr=}}
      local after_eq = peek(cursor, 3):sub(2, 3)
      if after_eq == '}}' then
        advance(cursor) -- consume =
        advance(cursor, 2) -- consume }}
        local expr_str = table.concat(expr)
        return expr_str, expr_str .. '=%s', tostring
      elseif peek(cursor, 2):sub(2, 2) == ':' and peek(cursor, 3):sub(3, 3) == '%' then
        advance(cursor) -- consume =
        advance(cursor) -- consume :
        local fmt = parse_format(cursor)
        local expr_str = table.concat(expr)
        local formatter = fmt:match('^ *%%.*s *$') and tostring or function(x)
          return x
        end
        return expr_str, expr_str .. '=' .. fmt, formatter
      else
        expr[#expr + 1] = advance(cursor)
      end
    elseif two_chars == ':%' then
      advance(cursor) -- consume :
      local fmt = parse_format(cursor)
      local formatter = fmt:match('^ *%%.*s *$') and tostring or function(x)
        return x
      end
      return table.concat(expr), fmt, formatter
    else
      expr[#expr + 1] = advance(cursor)
    end
  end

  error("unmatched '{{'", 2)
end

--- Parse the full interpolation string
local function parse(str)
  local cursor = create_cursor(str)
  local parts = {}
  local expressions = {}

  while not at_end(cursor) do
    local two_chars = peek(cursor, 2)

    if two_chars == '{{' then
      advance(cursor, 2)
      local expr, fmt, formatter = parse_expression(cursor)
      parts[#parts + 1] = fmt
      expressions[#expressions + 1] = { expr = expr, formatter = formatter }
    else
      parts[#parts + 1] = advance(cursor)
    end
  end

  return table.concat(parts), expressions
end

--- Main interpolation function
return function(str, values)
  local env
  if values == nil then
    assert(debug, 'debug library is not available, expansion names can only be provided as a table')
    local locals = get_locals()
    local upvals = get_upvalues()
    -- selene: allow(global_usage)
    env = setmetatable(locals, { __index = setmetatable(upvals, { __index = _G }) })
  else
    env = values
  end

  local fmt, expressions = parse(str)
  local results = {}

  for i, item in ipairs(expressions) do
    local value = env[item.expr] or eval(item.expr, env)
    results[i] = item.formatter(value)
  end

  return string.format(fmt, table.unpack(results))
end
