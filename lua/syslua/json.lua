--
-- json.lua
--
-- Copyright (c) 2020 rxi
--
-- Permission is hereby granted, free of charge, to any person obtaining a copy of
-- this software and associated documentation files (the "Software"), to deal in
-- the Software without restriction, including without limitation the rights to
-- use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
-- of the Software, and to permit persons to whom the Software is furnished to do
-- so, subject to the following conditions:
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
--

local json = { _version = '0.1.2' }

-------------------------------------------------------------------------------
-- Cached globals
-------------------------------------------------------------------------------

local type = type
local pairs = pairs
local ipairs = ipairs
local tostring = tostring
local tonumber = tonumber
local rawget = rawget
local next = next
local error = error
local select = select
local string_char = string.char
local string_byte = string.byte
local string_sub = string.sub
local string_gsub = string.gsub
local string_format = string.format
local string_match = string.match
local table_concat = table.concat
local math_floor = math.floor
local math_huge = math.huge

-------------------------------------------------------------------------------
-- Encode
-------------------------------------------------------------------------------

local encode

local escape_char_map = {
  ['\\'] = '\\',
  ['"'] = '"',
  ['\b'] = 'b',
  ['\f'] = 'f',
  ['\n'] = 'n',
  ['\r'] = 'r',
  ['\t'] = 't',
}

local escape_char_map_inv = { ['/'] = '/' }
for k, v in pairs(escape_char_map) do
  escape_char_map_inv[v] = k
end

local function escape_char(c)
  return '\\' .. (escape_char_map[c] or string_format('u%04x', string_byte(c)))
end

local function encode_nil(_)
  return 'null'
end

local function encode_table(val, stack)
  local res = {}
  local n = 0
  stack = stack or {}

  if stack[val] then
    error('circular reference')
  end

  stack[val] = true

  if rawget(val, 1) ~= nil or next(val) == nil then
    local count = 0
    for k in pairs(val) do
      if type(k) ~= 'number' then
        error('invalid table: mixed or invalid key types')
      end
      count = count + 1
    end
    if count ~= #val then
      error('invalid table: sparse array')
    end
    for _, v in ipairs(val) do
      n = n + 1
      res[n] = encode(v, stack)
    end
    stack[val] = nil
    return '[' .. table_concat(res, ',') .. ']'
  else
    for k, v in pairs(val) do
      if type(k) ~= 'string' then
        error('invalid table: mixed or invalid key types')
      end
      n = n + 1
      res[n] = encode(k, stack) .. ':' .. encode(v, stack)
    end
    stack[val] = nil
    return '{' .. table_concat(res, ',') .. '}'
  end
end

local function encode_string(val)
  return '"' .. string_gsub(val, '[%z\1-\31\\"]', escape_char) .. '"'
end

local function encode_number(val)
  if val ~= val or val <= -math_huge or val >= math_huge then
    error("unexpected number value '" .. tostring(val) .. "'")
  end
  return string_format('%.14g', val)
end

local type_func_map = {
  ['nil'] = encode_nil,
  ['table'] = encode_table,
  ['string'] = encode_string,
  ['number'] = encode_number,
  ['boolean'] = tostring,
}

encode = function(val, stack)
  local t = type(val)
  local f = type_func_map[t]
  if f then
    return f(val, stack)
  end
  error("unexpected type '" .. t .. "'")
end

function json.encode(val)
  return (encode(val))
end

-------------------------------------------------------------------------------
-- Decode
-------------------------------------------------------------------------------

local parse

local function create_set(...)
  local res = {}
  for i = 1, select('#', ...) do
    res[select(i, ...)] = true
  end
  return res
end

local space_chars = create_set(' ', '\t', '\r', '\n')
local delim_chars = create_set(' ', '\t', '\r', '\n', ']', '}', ',')
local escape_chars = create_set('\\', '/', '"', 'b', 'f', 'n', 'r', 't', 'u')
local literals = create_set('true', 'false', 'null')

local literal_map = {
  ['true'] = true,
  ['false'] = false,
  ['null'] = nil,
}

local function next_char(str, idx, set, negate)
  for i = idx, #str do
    if set[string_sub(str, i, i)] ~= negate then
      return i
    end
  end
  return #str + 1
end

local function decode_error(str, idx, msg)
  local line_count = 1
  local col_count = 1
  for i = 1, idx - 1 do
    col_count = col_count + 1
    if string_byte(str, i) == 10 then
      line_count = line_count + 1
      col_count = 1
    end
  end
  error(string_format('%s at line %d col %d', msg, line_count, col_count))
end

local function codepoint_to_utf8(n)
  if n <= 0x7f then
    return string_char(n)
  elseif n <= 0x7ff then
    return string_char(math_floor(n / 64) + 192, n % 64 + 128)
  elseif n <= 0xffff then
    return string_char(math_floor(n / 4096) + 224, math_floor(n % 4096 / 64) + 128, n % 64 + 128)
  elseif n <= 0x10ffff then
    return string_char(math_floor(n / 262144) + 240, math_floor(n % 262144 / 4096) + 128, math_floor(n % 4096 / 64) + 128, n % 64 + 128)
  end
  error(string_format("invalid unicode codepoint '%x'", n))
end

local function parse_unicode_escape(s)
  local n1 = tonumber(string_sub(s, 1, 4), 16)
  local n2 = tonumber(string_sub(s, 7, 10), 16)
  if n2 then
    return codepoint_to_utf8((n1 - 0xd800) * 0x400 + (n2 - 0xdc00) + 0x10000)
  else
    return codepoint_to_utf8(n1)
  end
end

local function parse_string(str, i)
  local res = {}
  local n = 0
  local j = i + 1
  local k = j

  while j <= #str do
    local x = string_byte(str, j)

    if x < 32 then
      decode_error(str, j, 'control character in string')
    elseif x == 92 then
      n = n + 1
      res[n] = string_sub(str, k, j - 1)
      j = j + 1
      local c = string_sub(str, j, j)
      if c == 'u' then
        local hex = string_match(str, '^[dD][89aAbB]%x%x\\u%x%x%x%x', j + 1)
          or string_match(str, '^%x%x%x%x', j + 1)
          or decode_error(str, j - 1, 'invalid unicode escape in string')
        n = n + 1
        res[n] = parse_unicode_escape(hex)
        j = j + #hex
      else
        if not escape_chars[c] then
          decode_error(str, j - 1, "invalid escape char '" .. c .. "' in string")
        end
        n = n + 1
        res[n] = escape_char_map_inv[c]
      end
      k = j + 1
    elseif x == 34 then
      n = n + 1
      res[n] = string_sub(str, k, j - 1)
      return table_concat(res), j + 1
    end

    j = j + 1
  end

  decode_error(str, i, 'expected closing quote for string')
end

local function parse_number(str, i)
  local x = next_char(str, i, delim_chars)
  local s = string_sub(str, i, x - 1)
  local num = tonumber(s)
  if not num then
    decode_error(str, i, "invalid number '" .. s .. "'")
  end
  return num, x
end

local function parse_literal(str, i)
  local x = next_char(str, i, delim_chars)
  local word = string_sub(str, i, x - 1)
  if not literals[word] then
    decode_error(str, i, "invalid literal '" .. word .. "'")
  end
  return literal_map[word], x
end

local function parse_array(str, i)
  local res = {}
  local n = 1
  i = i + 1
  while true do
    local x
    i = next_char(str, i, space_chars, true)
    if string_sub(str, i, i) == ']' then
      i = i + 1
      break
    end
    x, i = parse(str, i)
    res[n] = x
    n = n + 1
    i = next_char(str, i, space_chars, true)
    local chr = string_sub(str, i, i)
    i = i + 1
    if chr == ']' then
      break
    end
    if chr ~= ',' then
      decode_error(str, i, "expected ']' or ','")
    end
  end
  return res, i
end

local function parse_object(str, i)
  local res = {}
  i = i + 1
  while true do
    local key, val
    i = next_char(str, i, space_chars, true)
    if string_sub(str, i, i) == '}' then
      i = i + 1
      break
    end
    if string_sub(str, i, i) ~= '"' then
      decode_error(str, i, 'expected string for key')
    end
    key, i = parse(str, i)
    i = next_char(str, i, space_chars, true)
    if string_sub(str, i, i) ~= ':' then
      decode_error(str, i, "expected ':' after key")
    end
    i = next_char(str, i + 1, space_chars, true)
    val, i = parse(str, i)
    res[key] = val
    i = next_char(str, i, space_chars, true)
    local chr = string_sub(str, i, i)
    i = i + 1
    if chr == '}' then
      break
    end
    if chr ~= ',' then
      decode_error(str, i, "expected '}' or ','")
    end
  end
  return res, i
end

local char_func_map = {
  ['"'] = parse_string,
  ['0'] = parse_number,
  ['1'] = parse_number,
  ['2'] = parse_number,
  ['3'] = parse_number,
  ['4'] = parse_number,
  ['5'] = parse_number,
  ['6'] = parse_number,
  ['7'] = parse_number,
  ['8'] = parse_number,
  ['9'] = parse_number,
  ['-'] = parse_number,
  ['t'] = parse_literal,
  ['f'] = parse_literal,
  ['n'] = parse_literal,
  ['['] = parse_array,
  ['{'] = parse_object,
}

parse = function(str, idx)
  local chr = string_sub(str, idx, idx)
  local f = char_func_map[chr]
  if f then
    return f(str, idx)
  end
  decode_error(str, idx, "unexpected character '" .. chr .. "'")
end

function json.decode(str)
  if type(str) ~= 'string' then
    error('expected argument of type string, got ' .. type(str))
  end
  local res, idx = parse(str, next_char(str, 1, space_chars, true))
  idx = next_char(str, idx, space_chars, true)
  if idx <= #str then
    decode_error(str, idx, 'trailing garbage')
  end
  return res
end

return json
