--!strict
--! Instance paths: names from the service down, joined by "." with "\" and
--! "." escaped inside a name, so a path is unambiguous and round-trips.

local Paths = {}

function Paths.escape(name: string): string
	local escaped = name:gsub("[\\.]", function(c: string): string
		return "\\" .. c
	end)
	return escaped
end

function Paths.join(names: {string}): string
	local parts: {string} = {}
	for _, name in ipairs(names) do
		table.insert(parts, Paths.escape(name))
	end
	return table.concat(parts, ".")
end

function Paths.split(path: string): {string}?
	if path == "" then
		return nil
	end
	local names: {string} = {}
	local current: {string} = {}
	local i = 1
	local n = #path
	while i <= n do
		local c = path:sub(i, i)
		if c == "\\" then
			local nextChar = path:sub(i + 1, i + 1)
			if nextChar ~= "\\" and nextChar ~= "." then
				return nil
			end
			table.insert(current, nextChar)
			i += 2
		elseif c == "." then
			table.insert(names, table.concat(current))
			current = {}
			i += 1
		else
			table.insert(current, c)
			i += 1
		end
	end
	table.insert(names, table.concat(current))
	return names
end

function Paths.pathOf(instance: any, root: any?): string?
	local top: any = root
	if top == nil then
		top = (game :: any)
	end
	if instance == top then
		return nil
	end
	local names: {string} = {}
	local current: any = instance
	while current ~= top do
		if current == nil then
			return nil
		end
		table.insert(names, current.Name)
		current = current.Parent
	end
	local ordered: {string} = {}
	for i = #names, 1, -1 do
		table.insert(ordered, names[i])
	end
	return Paths.join(ordered)
end

function Paths.resolve(root: any, path: string): any?
	local names = Paths.split(path)
	if not names then
		return nil
	end
	local current: any = root
	for _, name in ipairs(names) do
		current = current:FindFirstChild(name)
		if current == nil then
			return nil
		end
	end
	return current
end

return Paths
