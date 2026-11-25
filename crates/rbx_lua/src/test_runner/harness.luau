-- The test runner's harness: TestEZ's describe/it/expect, with Jest-Lua's
-- matcher names and `.skip`/`.only` beside them. Rust passes the spec
-- module, a loader that compiles it with `env` in front of the globals,
-- a `report(kind, data)` callback, the run's options and `rearm()`, which
-- gives the next test a fresh time budget, and optionally `globals`, more
-- names for the spec (Local Run's `LocalRun`); see
-- `test_runner::run_file` for what each report means.
local spec, specName, loadSpec, report, options, rearm, globals = ...

local HOOKS = { "beforeAll", "afterAll", "beforeEach", "afterEach" }

local function newNode(kind, name, fn, parent, mode)
	local node = { kind = kind, name = name, fn = fn, parent = parent, children = {}, mode = mode, path = {} }
	for _, hook in HOOKS do
		node[hook] = {}
	end
	if parent then
		table.move(parent.path, 1, #parent.path, 1, node.path)
		table.insert(node.path, name)
	end
	return node
end

local root = newNode("describe", nil, nil, nil, nil)
local current = root
local anyFocus = false

-- An error and the stack it was raised on, as an xpcall handler.
local function trap(err)
	return { message = tostring(err), stack = debug.traceback() }
end

-- The spec line that is calling into the harness, for the dock to jump to.
local function callerLine()
	for level = 2, 20 do
		local source, line = debug.info(level, "sl")
		if source == nil then
			return nil
		end
		if source == specName then
			return line
		end
	end
	return nil
end

local function declare(kind, name, fn, mode)
	if type(name) ~= "string" then
		error(`{kind} expects a name, got {typeof(name)}`, 3)
	end
	local node = newNode(kind, name, fn, current, mode)
	node.line = callerLine()
	if mode == "focus" then
		anyFocus = true
	end
	table.insert(current.children, node)
	if kind == "describe" and fn then
		local outer = current
		current = node
		local ok, err = xpcall(fn, trap)
		current = outer
		if not ok then
			node.error = err
		end
	end
end

-- `describe("x", fn)` and `describe.skip`/`describe.only` (Jest-Lua).
local function declarer(kind)
	return setmetatable({
		skip = function(name, fn)
			declare(kind, name, fn, "skip")
		end,
		only = function(name, fn)
			declare(kind, name, fn, "focus")
		end,
	}, {
		-- Loren's `t.it(name, fn, { skip = true })` takes options too.
		__call = function(_, name, fn, opts)
			declare(kind, name, fn, if type(opts) == "table" and opts.skip then "skip" else nil)
		end,
	})
end

local function hook(which)
	return function(fn)
		table.insert(current[which], fn)
	end
end

local function show(value)
	if type(value) == "string" then
		return string.format("%q", value)
	end
	return `{tostring(value)} ({typeof(value)})`
end

local function deepEqual(a, b)
	if a == b then
		return true
	end
	if type(a) ~= "table" or type(b) ~= "table" then
		return false
	end
	for key, value in a do
		if not deepEqual(value, b[key]) then
			return false
		end
	end
	for key in b do
		if a[key] == nil then
			return false
		end
	end
	return true
end

local function length(value)
	if type(value) == "string" or type(value) == "table" then
		return #value
	end
	return nil
end

local CHAIN = { to = true, be = true, been = true, have = true, was = true, at = true }

local expectation

-- Matchers are called with a dot, as in TestEZ (`.to.equal(x)`) and
-- Jest-Lua (`.toBe(x)`). A failure is raised with no position: the
-- runner locates it from the stack instead.
local function matchers(value, negated)
	local function check(pass, positive, negative)
		if pass == negated then
			error(if negated then negative else positive, 0)
		end
	end
	local function throws(fragment)
		local ok, err = pcall(value)
		local message = tostring(err)
		local pass = not ok and (fragment == nil or string.find(message, fragment, 1, true) ~= nil)
		local wanted = if fragment then ` containing {show(fragment)}` else ""
		check(
			pass,
			if ok then `Expected the function to throw{wanted}, it returned` else `Expected an error{wanted}, got {show(message)}`,
			`Expected the function not to throw{wanted}, got {show(message)}`
		)
	end
	local function compare(op, word)
		return function(other)
			check(op(value, other), `Expected {show(value)} to be {word} {show(other)}`, `Expected {show(value)} not to be {word} {show(other)}`)
		end
	end
	local m = {}
	m.equal = function(other)
		check(value == other, `Expected {show(other)}, got {show(value)}`, `Expected anything but {show(other)}`)
	end
	m.toBe = m.equal
	m.toEqual = function(other)
		check(deepEqual(value, other), `Expected {show(other)} (deeply), got {show(value)}`, `Expected anything but {show(other)} (deeply)`)
	end
	m.ok = function()
		check(value ~= nil, `Expected a value, got nil`, `Expected nil, got {show(value)}`)
	end
	m.toBeTruthy = function()
		check(value ~= nil and value ~= false, `Expected a truthy value, got {show(value)}`, `Expected a falsy value, got {show(value)}`)
	end
	m.toBeFalsy = function()
		check(value == nil or value == false, `Expected a falsy value, got {show(value)}`, `Expected a truthy value, got {show(value)}`)
	end
	m.toBeNil = function()
		check(value == nil, `Expected nil, got {show(value)}`, `Expected a value, got nil`)
	end
	m.a = function(typeName)
		check(typeof(value) == typeName, `Expected a {typeName}, got {show(value)}`, `Expected anything but a {typeName}`)
	end
	m.an = m.a
	m.near = function(other, limit)
		limit = limit or 1e-7
		check(math.abs(value - other) <= limit, `Expected {show(value)} within {limit} of {show(other)}`, `Expected {show(value)} not within {limit} of {show(other)}`)
	end
	m.toBeCloseTo = function(other, digits)
		local limit = 10 ^ -(digits or 2) / 2
		check(math.abs(value - other) < limit, `Expected {show(value)} close to {show(other)}`, `Expected {show(value)} not close to {show(other)}`)
	end
	m.throw = throws
	m.toThrow = throws
	m.toContain = function(item)
		local found = false
		if type(value) == "string" then
			found = string.find(value, item, 1, true) ~= nil
		elseif type(value) == "table" then
			found = table.find(value, item) ~= nil
		end
		check(found, `Expected {show(value)} to contain {show(item)}`, `Expected {show(value)} not to contain {show(item)}`)
	end
	m.toHaveLength = function(n)
		check(length(value) == n, `Expected length {n}, got {tostring(length(value))}`, `Expected a length other than {n}`)
	end
	m.toBeGreaterThan = compare(function(a, b) return a > b end, "greater than")
	m.toBeGreaterThanOrEqual = compare(function(a, b) return a >= b end, "at least")
	m.toBeLessThan = compare(function(a, b) return a < b end, "less than")
	m.toBeLessThanOrEqual = compare(function(a, b) return a <= b end, "at most")
	return m
end

function expectation(value, negated)
	local m = matchers(value, negated)
	local self = {}
	return setmetatable(self, {
		__index = function(_, key)
			if CHAIN[key] then
				return self
			elseif key == "never" then
				return expectation(value, not negated)
			elseif m[key] then
				return m[key]
			end
			error(`'{tostring(key)}' is not a matcher this runner knows`, 2)
		end,
	})
end

local describe = declarer("describe")
local it = declarer("it")
local env = {
	describe = describe,
	it = it,
	test = it,
	xdescribe = describe.skip,
	fdescribe = describe.only,
	describeSKIP = describe.skip,
	describeFOCUS = describe.only,
	xit = it.skip,
	fit = it.only,
	itSKIP = it.skip,
	itFOCUS = it.only,
	SKIP = function()
		current.mode = "skip"
	end,
	FOCUS = function()
		current.mode = "focus"
		anyFocus = true
	end,
	beforeAll = hook("beforeAll"),
	afterAll = hook("afterAll"),
	beforeEach = hook("beforeEach"),
	afterEach = hook("afterEach"),
	expect = function(value)
		return expectation(value, false)
	end,
}

for name, value in globals or {} do
	env[name] = value
end

local loaded, loadError = xpcall(function()
	local body = loadSpec(spec, env)
	local value = body()
	-- TestEZ specs return a function that declares the tests; Jest-Lua
	-- ones declare them at the top level. Loren's `return function(t)`
	-- gets the same names as fields of `t`.
	if type(value) == "function" then
		value(env)
	end
end, trap)
if not loaded then
	root.error = loadError
end

local function prefixed(what, err)
	return { message = `{what} hook failed: {err.message}`, stack = err.stack }
end

local function result(path, status, failure, line, started)
	report("result", {
		path = path,
		status = status,
		message = failure and failure.message,
		stack = failure and failure.stack,
		line = line,
		duration = if started then os.clock() - started else 0,
	})
end

if options.plan then
	local function walk(node)
		for _, child in node.children do
			report("node", { kind = child.kind, path = child.path, line = child.line, mode = child.mode })
			walk(child)
		end
	end
	walk(root)
	if root.error then
		report("error", root.error)
	end
	report("done", { focus = anyFocus })
	return
end

local function selected(path)
	if options.filter and not string.find(table.concat(path, " "), options.filter, 1, true) then
		return false
	end
	if options.only then
		for i, part in options.only do
			if path[i] ~= nil and path[i] ~= part then
				return false
			end
		end
		return #path >= #options.only
	end
	return true
end

local function skipped(node)
	local focused = false
	local at = node
	while at do
		if at.mode == "skip" then
			return true
		end
		focused = focused or at.mode == "focus"
		at = at.parent
	end
	return options.focus and not focused
end

local function tests(node, out)
	for _, child in node.children do
		if child.kind == "it" then
			if selected(child.path) then
				table.insert(out, child)
			end
		else
			tests(child, out)
		end
	end
	return out
end

local function runHooks(list, what)
	local failure
	for _, fn in list do
		local ok, err = xpcall(fn, trap)
		if not ok and not failure then
			failure = prefixed(what, err)
		end
	end
	return failure
end

local function runTest(node)
	if node.fn == nil or skipped(node) then
		result(node.path, "skipped", nil, node.line)
		return
	end
	rearm()
	report("start", { path = node.path })
	local started = os.clock()
	local chain = {}
	local at = node.parent
	while at do
		table.insert(chain, 1, at)
		at = at.parent
	end
	local failure
	for _, scope in chain do
		failure = runHooks(scope.beforeEach, "beforeEach")
		if failure then
			break
		end
	end
	if not failure then
		local ok, err = xpcall(node.fn, trap)
		if not ok then
			failure = err
		end
	end
	for i = #chain, 1, -1 do
		failure = failure or runHooks(chain[i].afterEach, "afterEach")
	end
	result(node.path, if failure then "failed" else "passed", failure, node.line, started)
end

local function runDescribe(node)
	if node.error then
		if selected(node.path) or #tests(node, {}) > 0 then
			result(node.path, "failed", node.error, node.line)
		end
		return
	end
	local live = false
	for _, test in tests(node, {}) do
		live = live or (test.fn ~= nil and not skipped(test))
	end
	rearm()
	local failure = if live then runHooks(node.beforeAll, "beforeAll") else nil
	if failure then
		for _, test in tests(node, {}) do
			result(test.path, "failed", failure, test.line)
		end
	else
		for _, child in node.children do
			if child.kind == "it" then
				if selected(child.path) then
					runTest(child)
				end
			else
				runDescribe(child)
			end
		end
	end
	if live then
		rearm()
		local after = runHooks(node.afterAll, "afterAll")
		if after then
			local path = table.clone(node.path)
			table.insert(path, "afterAll")
			result(path, "failed", after, node.line)
		end
	end
end

runDescribe(root)
report("done", { focus = anyFocus })
