--!strict
-- Edge cases for the importer.
--[[ a long
comment ]]
local M = {}
export type Point = { x: number, y: number }
type Pair<T> = { first: T, second: T }

function M.a.b:c(x: number, ...: string): number
	return x
end

local function gen<T>(v: T): T
	return v
end

@native
local function fast(n: number): number
	return n * 2
end

local t = { 1, 2; 3, }
local k = { a = 1, ["b c"] = 2, nested = { 1, { 2 } }, }
local m = {
	-- lead
	x = 1, -- trailing
	y = 2,
}
local a, b = 1, 2
local c, d = f()
a, b = b, a
a += 1
a ..= "x"
local s = `hello {a} \{ \` world`
local e = 1 - 2 - 3
local f1 = a .. b .. c
local g = 2 ^ 3 ^ 2
local h = -a ^ 2
local i = not a == b
local j = if a then 1 elseif b then 2 else 3
local l = (a :: number)
local long = [==[
long string]==]

local function va(...)
	local x, y = ...
	return select("#", ...), x, y
end

local cb = function(v)
	return v + 1
end

for idx = 1, 10 do
	if idx == 2 then
		continue
	elseif idx == 5 then
		break
	else
		print(idx)
	end
end
for idx = 10, 1, -1 do end
for key, value in pairs(t) do
	print(key, value)
end
while a < 10 do
	a += 1
end
repeat
	a -= 1
until a < 0
do
	local scoped = 1
end

game:GetService("Players").PlayerAdded:Connect(function(player)
	print(player.Name)
end)

workspace.Part.Touched:Connect(function(hit)
	local n = hit.Name
	print(n, n)
end)

("abc"):upper()
t[1], t[2] = t[2], t[1]
local req = require(script.Parent.Module)
print("x") -- inline
print(#t, t[1], t.x)
local x1 = 1; local x2 = 2;
print(x1); print(x2)
local obj = a.b["c"].d:e("f", {
	g = 1, -- why
	-- own line
	h = function() return 2 end,
})
print "bare"
print { 1, 2 }
local ratio = 0x1F + 1e3 + 1_000 + .5 + 0b101
local nested = if a then (if b then 1 else 2) else 3
local fn2 = function(...) return ... end
local call = f(--[[c]] 1, 2 --[[d]])
local what = a and b or c
local n2 = #t + -x1 ~= 3
local str = "a\"b" .. 'c\'d' .. "tab\t"
local w = {
	[1] = "one",
	[f()] = "two";
	three = 3,
}
if a then print(1) end
if a then
	-- only a comment
end
while true do break end
return M
