-- Test suite for syslua.interpolation module
-- Uses {{}} delimiters to avoid confusion with shell ${} syntax
local f = require('syslua.interpolation')

local function assert_eq(actual, expected, msg)
  if actual ~= expected then
    error(string.format('%s\nExpected: %q\nActual: %q', msg or 'Assertion failed', expected, actual))
  end
end

local tests = {}

-- Basic interpolation tests
function tests.test_simple_variable()
  assert_eq(f('{{name}}', { name = 'world' }), 'world', 'Simple variable')
end

function tests.test_multiple_variables()
  assert_eq(f('{{a}} + {{b}} = {{c}}', { a = 1, b = 2, c = 3 }), '1 + 2 = 3', 'Multiple variables')
end

function tests.test_expression_evaluation()
  assert_eq(f('{{1 + 2}}', {}), '3', 'Expression evaluation')
end

function tests.test_nested_table_access()
  assert_eq(f('{{t.a.b}}', { t = { a = { b = 'nested' } } }), 'nested', 'Nested table access')
end

function tests.test_string_in_expression()
  assert_eq(f('{{string.upper(s)}}', { s = 'hello', string = string }), 'HELLO', 'String function')
end

function tests.test_math_expression()
  assert_eq(f('{{math.floor(x)}}', { x = 3.7, math = math }), '3', 'Math function')
end

-- Format specifier tests
function tests.test_format_integer()
  assert_eq(f('{{x:%d}}', { x = 42 }), '42', 'Integer format')
end

function tests.test_format_padded_integer()
  assert_eq(f('{{x:%05d}}', { x = 42 }), '00042', 'Padded integer')
end

function tests.test_format_float()
  assert_eq(f('{{x:%0.2f}}', { x = 3.14159 }), '3.14', 'Float format')
end

function tests.test_format_string()
  assert_eq(f('{{s:%s}}', { s = 'hello' }), 'hello', 'String format')
end

function tests.test_format_padded_string()
  assert_eq(f('{{s:%10s}}', { s = 'hello' }), '     hello', 'Padded string')
end

-- Debug expression syntax tests
function tests.test_debug_equals()
  assert_eq(f('{{x=}}', { x = 42 }), 'x=42', 'Debug equals')
end

function tests.test_debug_equals_with_format()
  assert_eq(f('{{x=:%05d}}', { x = 42 }), 'x=00042', 'Debug equals with format')
end

-- Edge cases
function tests.test_empty_string()
  assert_eq(f('', {}), '', 'Empty string')
end

function tests.test_no_interpolation()
  assert_eq(f('hello world', {}), 'hello world', 'No interpolation')
end

function tests.test_adjacent_interpolations()
  assert_eq(f('{{a}}{{b}}{{c}}', { a = 'x', b = 'y', c = 'z' }), 'xyz', 'Adjacent interpolations')
end

function tests.test_string_with_quotes()
  assert_eq(f([[{{s}}]], { s = "it's a test" }), "it's a test", 'String with quotes')
end

-- Single braces should pass through unchanged (important for shell compatibility)
function tests.test_single_braces_passthrough()
  assert_eq(f('echo ${HOME}', {}), 'echo ${HOME}', 'Single braces pass through')
end

function tests.test_single_brace_in_text()
  assert_eq(f('a { b } c', {}), 'a { b } c', 'Single braces in text')
end

function tests.test_mixed_braces()
  assert_eq(f('{{name}} uses ${HOME}', { name = 'test' }), 'test uses ${HOME}', 'Mixed braces')
end

-- Path construction tests (common use case)
function tests.test_path_construction()
  local home = '/home/user'
  local file = 'config.lua'
  assert_eq(
    f('{{home}}/.syslua/{{file}}', { home = home, file = file }),
    '/home/user/.syslua/config.lua',
    'Path construction'
  )
end

function tests.test_windows_path()
  local drive = 'C:'
  local dir = 'Users\\test'
  assert_eq(
    f('{{drive}}\\{{dir}}\\file.txt', { drive = drive, dir = dir }),
    'C:\\Users\\test\\file.txt',
    'Windows path'
  )
end

-- Shell command construction tests
function tests.test_simple_shell_command()
  local path = '/tmp/test'
  assert_eq(f('mkdir -p "{{path}}"', { path = path }), 'mkdir -p "/tmp/test"', 'Simple shell command')
end

function tests.test_shell_command_multiple_args()
  local src = '/src/file'
  local dst = '/dst/file'
  assert_eq(
    f('cp "{{src}}" "{{dst}}"', { src = src, dst = dst }),
    'cp "/src/file" "/dst/file"',
    'Shell command multiple args'
  )
end

function tests.test_shell_with_env_vars()
  local name = 'myfile'
  assert_eq(
    f('cp "{{name}}" "$HOME/{{name}}"', { name = name }),
    'cp "myfile" "$HOME/myfile"',
    'Shell with env vars preserved'
  )
end

function tests.test_shell_heredoc()
  local content = 'hello\nworld'
  local path = '/tmp/file'
  local expected = [[cat > "/tmp/file" << 'EOF'
hello
world
EOF]]
  local actual = f(
    [[cat > "{{path}}" << 'EOF'
{{content}}
EOF]],
    { path = path, content = content }
  )
  assert_eq(actual, expected, 'Shell heredoc')
end

-- Multiline string tests
function tests.test_multiline_template()
  local name = 'test'
  local version = '1.0'
  local expected = [[Package: test
Version: 1.0]]
  local actual = f(
    [[Package: {{name}}
Version: {{version}}]],
    { name = name, version = version }
  )
  assert_eq(actual, expected, 'Multiline template')
end

-- Error message tests (like raise_collision_error)
function tests.test_complex_error_message()
  local binary = 'rg'
  local pkg1 = 'ripgrep'
  local pkg2 = 'ripgrep-all'
  local expected = [[Conflict in 'rg'
  Package 1: ripgrep
  Package 2: ripgrep-all]]
  local actual = f(
    [[Conflict in '{{binary}}'
  Package 1: {{pkg1}}
  Package 2: {{pkg2}}]],
    { binary = binary, pkg1 = pkg1, pkg2 = pkg2 }
  )
  assert_eq(actual, expected, 'Complex error message')
end

-- Run all tests
local passed = 0
local failed = 0
local failures = {}

for name, test_fn in pairs(tests) do
  local ok, err = pcall(test_fn)
  if ok then
    passed = passed + 1
  else
    failed = failed + 1
    table.insert(failures, { name = name, error = err })
  end
end

-- Report results
print(string.format('Interpolation tests: %d passed, %d failed', passed, failed))

if #failures > 0 then
  print('\nFailures:')
  for _, failure in ipairs(failures) do
    print(string.format('  %s: %s', failure.name, failure.error))
  end
  os.exit(1)
end

print('All interpolation tests passed!')
