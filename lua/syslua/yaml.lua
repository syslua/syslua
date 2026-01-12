--
-- yaml.lua
--
-- Simple YAML parser for SysLua configuration files.
--

local yaml = { version = '1.3' }

-------------------------------------------------------------------------------
-- Cached globals
-------------------------------------------------------------------------------

local type = type
local pairs = pairs
local ipairs = ipairs
local tostring = tostring
local tonumber = tonumber
local error = error
local string_sub = string.sub
local string_rep = string.rep
local string_format = string.format
local string_match = string.match
local string_gsub = string.gsub
local table_concat = table.concat
local table_sort = table.sort

-------------------------------------------------------------------------------
-- Utility functions
-------------------------------------------------------------------------------

local function table_print_value(value, indent, done)
  indent = indent or 0
  done = done or {}

  if type(value) == 'table' and not done[value] then
    done[value] = true

    local list = {}
    local n = 0
    for key in pairs(value) do
      n = n + 1
      list[n] = key
    end
    table_sort(list, function(a, b)
      return tostring(a) < tostring(b)
    end)

    local parts = {}
    local pn = 0
    pn = pn + 1
    parts[pn] = '{\n'

    for i, key in ipairs(list) do
      local comma = i == n and '' or ','
      local key_rep
      if type(key) == 'number' then
        key_rep = key
      else
        key_rep = string_format('%q', tostring(key))
      end
      pn = pn + 1
      parts[pn] = string_format(
        '%s[%s] = %s%s\n',
        string_rep(' ', indent + 2),
        key_rep,
        table_print_value(value[key], indent + 2, done),
        comma
      )
    end

    pn = pn + 1
    parts[pn] = string_rep(' ', indent) .. '}'

    done[value] = false
    return table_concat(parts)
  elseif type(value) == 'string' then
    return string_format('%q', value)
  else
    return tostring(value)
  end
end

local function string_trim(s, what)
  what = what or ' '
  return string_gsub(s, '^[' .. what .. ']*(.-)[' .. what .. ']*$', '%1')
end

local function context(str)
  if type(str) ~= 'string' then
    return ''
  end
  str = string_gsub(string_sub(str, 1, 25), '\n', '\\n')
  str = string_gsub(str, '"', '\\"')
  return ', near "' .. str .. '"'
end

-------------------------------------------------------------------------------
-- Token patterns
-------------------------------------------------------------------------------

local function word(w)
  return '^(' .. w .. ')([%s$%c])'
end

local token_patterns = {
  { type = 'comment', pattern = '^#[^\n]*' },
  { type = 'indent', pattern = '^\n( *)' },
  { type = 'space', pattern = '^ +' },
  { type = 'true', pattern = word('enabled'), const = true, value = true },
  { type = 'true', pattern = word('true'), const = true, value = true },
  { type = 'true', pattern = word('yes'), const = true, value = true },
  { type = 'true', pattern = word('on'), const = true, value = true },
  { type = 'false', pattern = word('disabled'), const = true, value = false },
  { type = 'false', pattern = word('false'), const = true, value = false },
  { type = 'false', pattern = word('no'), const = true, value = false },
  { type = 'false', pattern = word('off'), const = true, value = false },
  { type = 'null', pattern = word('null'), const = true },
  { type = 'null', pattern = word('Null'), const = true },
  { type = 'null', pattern = word('NULL'), const = true },
  { type = 'null', pattern = word('~'), const = true },
  { type = 'id', pattern = '^"([^"]-)" *(:[%s%c])' },
  { type = 'id', pattern = "^'([^']-)' *(:[%s%c])" },
  { type = 'string', pattern = '^"([^"]-)"', force_text = true },
  { type = 'string', pattern = "^'([^']-)'", force_text = true },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)%s+(%d%d?):(%d%d):(%d%d)%s+(%-?%d%d?):(%d%d)' },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)%s+(%d%d?):(%d%d):(%d%d)%s+(%-?%d%d?)' },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)%s+(%d%d?):(%d%d):(%d%d)' },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)%s+(%d%d?):(%d%d)' },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)%s+(%d%d?)' },
  { type = 'timestamp', pattern = '^(%d%d%d%d)-(%d%d?)-(%d%d?)' },
  { type = 'doc', pattern = '^%-%-%-[^%c]*' },
  { type = ',', pattern = '^,' },
  { type = 'string', pattern = '^%b{} *[^,%c]+', noinline = true },
  { type = '{', pattern = '^{' },
  { type = '}', pattern = '^}' },
  { type = 'string', pattern = '^%b[] *[^,%c]+', noinline = true },
  { type = '[', pattern = '^%[' },
  { type = ']', pattern = '^%]' },
  { type = '-', pattern = '^%-', noinline = true },
  { type = ':', pattern = '^:' },
  { type = 'pipe', pattern = '^(|)(%d*[+%-]?)', sep = '\n' },
  { type = 'pipe', pattern = '^(>)(%d*[+%-]?)', sep = ' ' },
  { type = 'id', pattern = '^([%w][%w %-_]*)(:[%s%c])' },
  { type = 'string', pattern = '^[^%c]+', noinline = true },
  { type = 'string', pattern = '^[^,%]}%c ]+' },
}

-------------------------------------------------------------------------------
-- Tokenizer
-------------------------------------------------------------------------------

local function tokenize(str)
  local row = 0
  local indents = 0
  local last_indents = 0
  local indent_amount = 0
  local inline = false
  local stack = {}
  local stack_n = 0

  str = string_gsub(str, '\r\n', '\010')

  while #str > 0 do
    local token
    local ignore = false

    for i = 1, #token_patterns do
      local pat = token_patterns[i]
      local captures

      if not inline or not pat.noinline then
        captures = { string_match(str, pat.pattern) }
      end

      if captures and #captures > 0 then
        token = {
          type = pat.type,
          captures = captures,
          input = string_sub(str, 1, 25),
          const = pat.const,
          value = pat.value,
          force_text = pat.force_text,
          sep = pat.sep,
        }

        local str2 = string_gsub(str, pat.pattern, '', 1)
        token.raw = string_sub(str, 1, #str - #str2)
        str = str2

        if token.type == '{' or token.type == '[' then
          inline = true
        elseif token.const then
          str = token.captures[2] .. str
          token.raw = string_sub(token.raw, 1, #token.raw - #token.captures[2])
        elseif token.type == 'id' then
          str = token.captures[2] .. str
          token.raw = string_sub(token.raw, 1, #token.raw - #token.captures[2])
          token.captures[1] = string_trim(token.captures[1])
        elseif token.type == 'string' then
          local snip = token.captures[1]
          if not token.force_text then
            if string_match(snip, '^(-?%d+%.%d+)$') or string_match(snip, '^(-?%d+)$') then
              token.type = 'number'
            end
          end
        elseif token.type == 'comment' then
          ignore = true
        elseif token.type == 'indent' then
          row = row + 1
          inline = false
          last_indents = indents

          if indent_amount == 0 then
            indent_amount = #token.captures[1]
          end

          if indent_amount ~= 0 then
            indents = #token.captures[1] / indent_amount
          else
            indents = 0
          end

          if indents == last_indents then
            ignore = true
          elseif indents > last_indents + 2 then
            error(
              'SyntaxError: invalid indentation, got '
                .. tostring(indents)
                .. ' instead of '
                .. tostring(last_indents)
                .. context(token.input)
            )
          elseif indents > last_indents + 1 then
            stack_n = stack_n + 1
            stack[stack_n] = token
          elseif indents < last_indents then
            local input = token.input
            token = { type = 'dedent', captures = { '' }, input = input }
            while last_indents > indents + 1 do
              last_indents = last_indents - 1
              stack_n = stack_n + 1
              stack[stack_n] = token
            end
          end
        end

        token.row = row
        break
      end
    end

    if not ignore then
      if token then
        stack_n = stack_n + 1
        stack[stack_n] = token
      else
        error('SyntaxError' .. context(str))
      end
    end
  end

  return stack
end

-------------------------------------------------------------------------------
-- Parser
-------------------------------------------------------------------------------

local Parser = {}
Parser.__index = Parser

function Parser.new(tokens)
  local self = setmetatable({}, Parser)
  self.tokens = tokens
  self.parse_stack = {}
  self.refs = {}
  self.current = 0
  return self
end

function Parser:peek(offset)
  offset = offset or 1
  return self.tokens[offset + self.current]
end

function Parser:advance()
  self.current = self.current + 1
  return self.tokens[self.current]
end

function Parser:advanceValue()
  return self:advance().captures[1]
end

function Parser:peekType(val, offset)
  local token = self:peek(offset)
  return token and token.type == val
end

function Parser:accept(tok_type)
  if self:peekType(tok_type) then
    return self:advance()
  end
end

function Parser:expect(tok_type, msg)
  local token = self:accept(tok_type)
  if token then
    return token
  end
  local peek = self:peek()
  error(msg .. context(peek and peek.input or nil))
end

function Parser:expectDedent(msg)
  if self:accept('dedent') then
    return true
  end
  if self:peek() == nil then
    return true
  end
  local peek = self:peek()
  error(msg .. context(peek and peek.input or nil))
end

function Parser:ignore(items)
  local advanced = true
  while advanced do
    advanced = false
    for _, v in pairs(items) do
      if self:peekType(v) then
        self:advance()
        advanced = true
      end
    end
  end
end

function Parser:ignoreSpace()
  self:ignore({ 'space' })
end

function Parser:ignoreWhitespace()
  self:ignore({ 'space', 'indent', 'dedent' })
end

function Parser:inline()
  local current = self:peek(0)
  if not current then
    return {}, 0
  end

  local inline_tokens = {}
  local i = 0

  while self:peek(i) and not self:peekType('indent', i) and current.row == self:peek(i).row do
    inline_tokens[self:peek(i).type] = true
    i = i - 1
  end
  return inline_tokens, -i
end

function Parser:isInline()
  local _, count = self:inline()
  return count > 0
end

function Parser:parent(level)
  level = level or 1
  return self.parse_stack[#self.parse_stack - level]
end

function Parser:parse()
  local ref = nil
  if self:peekType('string') and not self:peek().force_text then
    local char = string_sub(self:peek().captures[1], 1, 1)
    if char == '&' then
      ref = string_sub(self:peek().captures[1], 2)
      self:advanceValue()
      self:ignoreSpace()
    elseif char == '*' then
      ref = string_sub(self:peek().captures[1], 2)
      return self.refs[ref]
    end
  end

  local result
  local c = {
    indent = self:accept('indent') and 1 or 0,
    token = self:peek(),
  }
  self.parse_stack[#self.parse_stack + 1] = c

  local token_type = c.token.type

  if token_type == 'doc' then
    result = self:parseDoc()
  elseif token_type == '-' then
    result = self:parseList()
  elseif token_type == '{' then
    result = self:parseInlineHash()
  elseif token_type == '[' then
    result = self:parseInlineList()
  elseif token_type == 'id' then
    result = self:parseHash()
  elseif token_type == 'string' then
    result = self:parseString()
  elseif token_type == 'timestamp' then
    result = self:parseTimestamp()
  elseif token_type == 'number' then
    result = tonumber(self:advanceValue())
  elseif token_type == 'pipe' then
    result = self:parsePipe()
  elseif c.token.const == true then
    self:advanceValue()
    result = c.token.value
  else
    error("ParseError: unexpected token '" .. token_type .. "'" .. context(c.token.input))
  end

  self.parse_stack[#self.parse_stack] = nil

  while c.indent > 0 do
    c.indent = c.indent - 1
    local term = 'term ' .. c.token.type .. ": '" .. c.token.captures[1] .. "'"
    self:expectDedent('last ' .. term .. ' is not properly dedented')
  end

  if ref then
    self.refs[ref] = result
  end
  return result
end

function Parser:parseDoc()
  self:accept('doc')
  return self:parse()
end

function Parser:parseString()
  if self:isInline() then
    local result = self:advanceValue()

    -- Handle: - a: this looks
    --           flowing: but is
    --           no: string
    local types = self:inline()
    if types['id'] and types['-'] then
      if not self:peekType('indent') or not self:peekType('indent', 2) then
        return result
      end
    end

    -- Handle flowing strings
    if self:peekType('indent') then
      self:expect('indent', 'text block needs to start with indent')
      local addtl = self:accept('indent')
      result = result .. '\n' .. self:parseTextBlock('\n')
      self:expectDedent('text block ending dedent missing')
      if addtl then
        self:expectDedent('text block ending dedent missing')
      end
    end
    return result
  else
    return self:parseTextBlock('\n')
  end
end

function Parser:parsePipe()
  local pipe = self:expect('pipe')
  self:expect('indent', 'text block needs to start with indent')
  local result = self:parseTextBlock(pipe.sep)
  self:expectDedent('text block ending dedent missing')
  return result
end

function Parser:parseTextBlock(sep)
  local token = self:advance()
  local parts = {}
  local pn = 1
  parts[pn] = string_trim(token.raw, '\n')
  local indent_count = 0

  while self:peek() ~= nil and (indent_count > 0 or not self:peekType('dedent')) do
    local newtoken = self:advance()
    while token.row < newtoken.row do
      pn = pn + 1
      parts[pn] = sep
      token.row = token.row + 1
    end
    if newtoken.type == 'indent' then
      indent_count = indent_count + 1
    elseif newtoken.type == 'dedent' then
      indent_count = indent_count - 1
    else
      pn = pn + 1
      parts[pn] = string_trim(newtoken.raw, '\n')
    end
  end

  return table_concat(parts)
end

function Parser:parseHash(hash)
  hash = hash or {}
  local indent_count = 0

  if self:isInline() then
    local id = self:advanceValue()
    self:expect(':', 'expected semi-colon after id')
    self:ignoreSpace()
    if self:accept('indent') then
      indent_count = indent_count + 1
      hash[id] = self:parse()
    else
      hash[id] = self:parse()
      if self:accept('indent') then
        indent_count = indent_count + 1
      end
    end
    self:ignoreSpace()
  end

  while self:peekType('id') do
    local id = self:advanceValue()
    self:expect(':', 'expected semi-colon after id')
    self:ignoreSpace()
    hash[id] = self:parse()
    self:ignoreSpace()
  end

  while indent_count > 0 do
    self:expectDedent('expected dedent')
    indent_count = indent_count - 1
  end

  return hash
end

function Parser:parseInlineHash()
  local hash = {}
  local i = 0

  self:accept('{')
  while not self:accept('}') do
    self:ignoreSpace()
    if i > 0 then
      self:expect(',', 'expected comma')
    end

    self:ignoreWhitespace()
    if self:peekType('id') then
      local id = self:advanceValue()
      if id then
        self:expect(':', 'expected semi-colon after id')
        self:ignoreSpace()
        hash[id] = self:parse()
        self:ignoreWhitespace()
      end
    end

    i = i + 1
  end
  return hash
end

function Parser:parseList()
  local list = {}
  local n = 0
  while self:accept('-') do
    self:ignoreSpace()
    n = n + 1
    list[n] = self:parse()
    self:ignoreSpace()
  end
  return list
end

function Parser:parseInlineList()
  local list = {}
  local n = 0
  local i = 0
  self:accept('[')
  while not self:accept(']') do
    self:ignoreSpace()
    if i > 0 then
      self:expect(',', 'expected comma')
    end
    self:ignoreSpace()
    n = n + 1
    list[n] = self:parse()
    self:ignoreSpace()
    i = i + 1
  end
  return list
end

function Parser:parseTimestamp()
  local cap = self:advance().captures

  return os.time({
    year = cap[1],
    month = cap[2],
    day = cap[3],
    hour = cap[4] or 0,
    min = cap[5] or 0,
    sec = cap[6] or 0,
    isdst = false,
  }) - os.time({ year = 1970, month = 1, day = 1, hour = 8 })
end

-------------------------------------------------------------------------------
-- Public API
-------------------------------------------------------------------------------

function yaml.eval(str)
  return Parser.new(tokenize(str)):parse()
end

function yaml.dump(tt)
  print('return ' .. table_print_value(tt))
end

yaml.tokenize = tokenize

return yaml
