-- Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
--
-- The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
--
-- THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

local type = type
local pairs = pairs
local tostring = tostring
local tonumber = tonumber
local error = error
local string_sub = string.sub
local string_char = string.char
local string_match = string.match
local string_gmatch = string.gmatch
local string_gsub = string.gsub
local string_len = string.len
local string_lower = string.lower
local string_rep = string.rep
local table_concat = table.concat
local table_insert = table.insert
local math_floor = math.floor

local toml = {
  version = 0.40,
  strict = true,
}

-- converts TOML data into a lua table
toml.parse = function(toml_string, options)
  options = options or {}
  local strict = (options.strict ~= nil and options.strict or toml.strict)

  local ws = '[\009\032]'
  local nl = '[\10\13\10]'
  local str_len = string_len(toml_string)

  local cursor = 1
  local out = {}
  local obj = out

  local function char(n)
    n = n or 0
    local pos = cursor + n
    return string_sub(toml_string, pos, pos)
  end

  local function step(n)
    n = n or 1
    cursor = cursor + n
  end

  local function skipWhitespace()
    while string_match(char(), ws) do
      step()
    end
  end

  local function trim(str)
    return string_gsub(str, '^%s*(.-)%s*$', '%1')
  end

  local function err(message, strictOnly)
    if not strictOnly or (strictOnly and strict) then
      local line = 1
      local c = 0
      for l in string_gmatch(toml_string, '(.-)' .. nl) do
        c = c + string_len(l)
        if c >= cursor then
          break
        end
        line = line + 1
      end
      error('TOML: ' .. message .. ' on line ' .. line .. '.', 4)
    end
  end

  local function bounds()
    return cursor <= str_len
  end

  local escape_chars = {
    b = '\b',
    t = '\t',
    n = '\n',
    f = '\f',
    r = '\r',
    ['"'] = '"',
    ['\\'] = '\\',
  }

  local function codepoint_to_utf8(cp)
    local bytemarkers = { { 0x7ff, 192 }, { 0xffff, 224 }, { 0x1fffff, 240 } }
    if cp < 128 then
      return string_char(cp)
    end
    local charbytes = {}
    for bytes, vals in pairs(bytemarkers) do
      if cp <= vals[1] then
        for b = bytes + 1, 2, -1 do
          local mod = cp % 64
          cp = (cp - mod) / 64
          charbytes[b] = string_char(128 + mod)
        end
        charbytes[1] = string_char(vals[2] + cp)
        break
      end
    end
    return table_concat(charbytes)
  end

  local function parseString()
    local quoteType = char()
    local c1, c2 = char(1), char(2)
    local multiline = (c1 == c2 and c1 == quoteType)

    local parts = {}
    local n = 0
    local is_empty = true

    step(multiline and 3 or 1)

    while bounds() do
      local c = char()

      if multiline and string_match(c, nl) and is_empty then
        step()
      elseif c == quoteType then
        if multiline then
          if char(1) == char(2) and char(1) == quoteType then
            step(3)
            break
          else
            n = n + 1
            parts[n] = c
            step()
          end
        else
          step()
          break
        end
      elseif string_match(c, nl) and not multiline then
        err('Single-line string cannot contain line break')
      elseif quoteType == '"' and c == '\\' then
        local c1_esc = char(1)
        if multiline and string_match(c1_esc, nl) then
          step(1)
          while bounds() do
            local ch = char()
            if not string_match(ch, ws) and not string_match(ch, nl) then
              break
            end
            step()
          end
        elseif escape_chars[c1_esc] then
          n = n + 1
          parts[n] = escape_chars[c1_esc]
          is_empty = false
          step(2)
        elseif c1_esc == 'u' then
          step()
          local uni = char(1) .. char(2) .. char(3) .. char(4)
          step(5)
          local uni_num = tonumber(uni, 16)
          if (uni_num >= 0 and uni_num <= 0xd7ff) and not (uni_num >= 0xe000 and uni_num <= 0x10ffff) then
            n = n + 1
            parts[n] = codepoint_to_utf8(uni_num)
            is_empty = false
          else
            err('Unicode escape is not a Unicode scalar')
          end
        elseif c1_esc == 'U' then
          step()
          local uni = char(1) .. char(2) .. char(3) .. char(4) .. char(5) .. char(6) .. char(7) .. char(8)
          step(9)
          local uni_num = tonumber(uni, 16)
          if (uni_num >= 0 and uni_num <= 0xd7ff) and not (uni_num >= 0xe000 and uni_num <= 0x10ffff) then
            n = n + 1
            parts[n] = codepoint_to_utf8(uni_num)
            is_empty = false
          else
            err('Unicode escape is not a Unicode scalar')
          end
        else
          err('Invalid escape')
        end
      else
        n = n + 1
        parts[n] = c
        is_empty = false
        step()
      end
    end

    return { value = table_concat(parts), type = 'string' }
  end

  local function parseNumber()
    local num_parts = {}
    local num_n = 0
    local exp_parts = {}
    local exp_n = 0
    local has_exp = false
    local date = false

    while bounds() do
      local c = char()
      if string_match(c, '[%+%-%.eE_0-9]') then
        if not has_exp then
          if string_lower(c) == 'e' then
            has_exp = true
          elseif c ~= '_' then
            num_n = num_n + 1
            num_parts[num_n] = c
          end
        elseif string_match(c, '[%+%-0-9]') then
          exp_n = exp_n + 1
          exp_parts[exp_n] = c
        else
          err('Invalid exponent')
        end
      elseif string_match(c, ws) or c == '#' or string_match(c, nl) or c == ',' or c == ']' or c == '}' then
        break
      elseif c == 'T' or c == 'Z' then
        date = true
        while bounds() do
          local dc = char()
          if dc == ',' or dc == ']' or dc == '#' or string_match(dc, nl) or string_match(dc, ws) then
            break
          end
          num_n = num_n + 1
          num_parts[num_n] = dc
          step()
        end
      else
        err('Invalid number')
      end
      step()
    end

    local num_str = table_concat(num_parts)

    if date then
      return { value = num_str, type = 'date' }
    end

    local float = string_match(num_str, '%.')
    local exp = has_exp and tonumber(table_concat(exp_parts)) or 0
    local num = tonumber(num_str)

    if not float then
      return { value = math_floor(num * 10 ^ exp), type = 'int' }
    end

    return { value = num * 10 ^ exp, type = 'float' }
  end

  local parseArray, getValue

  function parseArray()
    step()
    skipWhitespace()

    local arrayType
    local array = {}
    local arr_n = 0

    while bounds() do
      local c = char()
      if c == ']' then
        break
      elseif string_match(c, nl) then
        step()
        skipWhitespace()
      elseif c == '#' then
        while bounds() and not string_match(char(), nl) do
          step()
        end
      else
        local v = getValue()
        if not v then
          break
        end

        if arrayType == nil then
          arrayType = v.type
        elseif arrayType ~= v.type then
          err('Mixed types in array', true)
        end

        arr_n = arr_n + 1
        array[arr_n] = v.value

        if char() == ',' then
          step()
        end
        skipWhitespace()
      end
    end
    step()

    return { value = array, type = 'array' }
  end

  local function parseInlineTable()
    step()

    local key_parts = {}
    local key_n = 0
    local quoted = false
    local tbl = {}

    while bounds() do
      local c = char()
      if c == '}' then
        break
      elseif c == "'" or c == '"' then
        key_parts = { parseString().value }
        key_n = 1
        quoted = true
      elseif c == '=' then
        local key = quoted and key_parts[1] or trim(table_concat(key_parts))

        step()
        skipWhitespace()

        if string_match(char(), nl) then
          err('Newline in inline table')
        end

        local v = getValue().value
        tbl[key] = v

        skipWhitespace()

        local c2 = char()
        if c2 == ',' then
          step()
        elseif string_match(c2, nl) then
          err('Newline in inline table')
        end

        quoted = false
        key_parts = {}
        key_n = 0
      else
        key_n = key_n + 1
        key_parts[key_n] = c
        step()
      end
    end
    step()

    return { value = tbl, type = 'array' }
  end

  local function parseBoolean()
    local v
    if string_sub(toml_string, cursor, cursor + 3) == 'true' then
      step(4)
      v = { value = true, type = 'boolean' }
    elseif string_sub(toml_string, cursor, cursor + 4) == 'false' then
      step(5)
      v = { value = false, type = 'boolean' }
    else
      err('Invalid primitive')
    end

    skipWhitespace()
    if char() == '#' then
      while not string_match(char(), nl) do
        step()
      end
    end

    return v
  end

  function getValue()
    local c = char()
    if c == '"' or c == "'" then
      return parseString()
    elseif string_match(c, '[%+%-0-9]') then
      return parseNumber()
    elseif c == '[' then
      return parseArray()
    elseif c == '{' then
      return parseInlineTable()
    else
      return parseBoolean()
    end
  end

  local quotedKey = false
  local buf_parts = {}
  local buf_n = 0

  local function get_buffer()
    return table_concat(buf_parts)
  end

  local function clear_buffer()
    buf_parts = {}
    buf_n = 0
  end

  local function append_buffer(c)
    buf_n = buf_n + 1
    buf_parts[buf_n] = c
  end

  while cursor <= str_len do
    local c = char()

    if c == '#' then
      while not string_match(char(), nl) do
        step()
      end
      c = char()
    end

    if c == '=' then
      step()
      skipWhitespace()

      local key_str = trim(get_buffer())
      ---@type string|number
      local key = key_str

      if string_match(key_str, '^[0-9]*$') and not quotedKey then
        key = tonumber(key_str) or key_str
      end

      if (not key or key == '') and not quotedKey then
        err('Empty key name')
      end

      local v = getValue()
      if v and v.value ~= nil then
        if obj[key] then
          err('Cannot redefine key "' .. tostring(key) .. '"', true)
        end
        obj[key] = v.value
      end

      clear_buffer()
      quotedKey = false

      skipWhitespace()
      if char() == '#' then
        while bounds() and not string_match(char(), nl) do
          step()
        end
      end

      if not string_match(char(), nl) and cursor < str_len then
        err('Invalid primitive')
      end
    elseif c == '[' then
      clear_buffer()
      step()
      local tableArray = false

      if char() == '[' then
        tableArray = true
        step()
      end

      obj = out

      local function processKey(isLast)
        isLast = isLast or false
        local key = trim(get_buffer())

        if not quotedKey and key == '' then
          err('Empty table name')
        end

        if isLast and obj[key] and not tableArray and #obj[key] > 0 then
          err('Cannot redefine table', true)
        end

        if tableArray then
          if obj[key] then
            obj = obj[key]
            if isLast then
              table_insert(obj, {})
            end
            obj = obj[#obj]
          else
            obj[key] = {}
            obj = obj[key]
            if isLast then
              table_insert(obj, {})
              obj = obj[1]
            end
          end
        else
          obj[key] = obj[key] or {}
          obj = obj[key]
        end
      end

      while bounds() do
        local ch = char()
        if ch == ']' then
          if tableArray then
            if char(1) ~= ']' then
              err('Mismatching brackets')
            else
              step()
            end
          end
          step()

          processKey(true)
          clear_buffer()
          break
        elseif ch == '"' or ch == "'" then
          buf_parts = { parseString().value }
          buf_n = 1
          quotedKey = true
        elseif ch == '.' then
          step()
          processKey()
          clear_buffer()
        else
          append_buffer(ch)
          step()
        end
      end

      clear_buffer()
      quotedKey = false
    elseif c == '"' or c == "'" then
      buf_parts = { parseString().value }
      buf_n = 1
      quotedKey = true
    elseif not string_match(c, nl) then
      append_buffer(c)
    end

    step()
  end

  return out
end

toml.encode = function(tbl)
  local output = {}
  local out_n = 0

  local cache = {}
  local cache_n = 0

  local function emit(s)
    out_n = out_n + 1
    output[out_n] = s
  end

  local function encode_table(t)
    for k, v in pairs(t) do
      local vtype = type(v)
      if vtype == 'boolean' or vtype == 'number' then
        emit(k .. ' = ' .. tostring(v) .. '\n')
      elseif vtype == 'string' then
        local quote = '"'
        v = string_gsub(v, '\\', '\\\\')

        if string_match(v, '^\n(.*)$') then
          quote = string_rep(quote, 3)
          v = '\\n' .. v
        elseif string_match(v, '\n') then
          quote = string_rep(quote, 3)
        end

        v = string_gsub(v, '\b', '\\b')
        v = string_gsub(v, '\t', '\\t')
        v = string_gsub(v, '\f', '\\f')
        v = string_gsub(v, '\r', '\\r')
        v = string_gsub(v, '"', '\\"')
        v = string_gsub(v, '/', '\\/')
        emit(k .. ' = ' .. quote .. v .. quote .. '\n')
      elseif vtype == 'table' then
        local array, arrayTable = true, true
        local first = {}
        for kk, vv in pairs(v) do
          if type(kk) ~= 'number' then
            array = false
          end
          if type(vv) ~= 'table' then
            v[kk] = nil
            first[kk] = vv
            arrayTable = false
          end
        end

        if array then
          if arrayTable then
            cache_n = cache_n + 1
            cache[cache_n] = k
            for _, vv in pairs(v) do
              emit('[[' .. table_concat(cache, '.') .. ']]\n')
              for k3, v3 in pairs(vv) do
                if type(v3) ~= 'table' then
                  vv[k3] = nil
                  first[k3] = v3
                end
              end
              encode_table(first)
              encode_table(vv)
            end
            cache[cache_n] = nil
            cache_n = cache_n - 1
          else
            emit(k .. ' = [\n')
            for _, vv in pairs(first) do
              emit(tostring(vv) .. ',\n')
            end
            emit(']\n')
          end
        else
          cache_n = cache_n + 1
          cache[cache_n] = k
          emit('[' .. table_concat(cache, '.') .. ']\n')
          encode_table(first)
          encode_table(v)
          cache[cache_n] = nil
          cache_n = cache_n - 1
        end
      end
    end
  end

  encode_table(tbl)

  local result = table_concat(output)
  return string_sub(result, 1, -2)
end

return toml
