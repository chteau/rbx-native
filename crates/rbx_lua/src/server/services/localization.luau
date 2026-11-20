--!nonstrict
-- LocalizationService, LocalizationTable and Translator in Local Run. A
-- table's entries are its saved `Contents` (the JSON a place file keeps:
-- key, context, examples, source and a value per locale), read and
-- written through the host. A Translator looks in every LocalizationTable
-- under LocalizationService (or its own table's), by source text or by
-- key, falling back from a regional locale to its language, then to the
-- source. Automatic translation of GUI text stays off: only what scripts
-- ask for is translated.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function typeName(value)
	if value == nil then
		return "nil"
	end
	if type(value) == "number" then
		return "double"
	end
	return typeof(value)
end

local function str(value)
	if type(value) ~= "string" then
		fail("Unable to cast " .. typeName(value) .. " to string")
	end
	return value
end

local function isPlayer(value)
	return typeof(value) == "Instance" and value:IsA("Player")
end

-- A table's entries as stored (lower-case fields).
local function load(tbl)
	local raw = host.raw(host.id(tbl), "Contents")
	if raw == nil or raw == "" then
		return {}
	end
	local ok, list = pcall(host.decode, raw)
	return if ok and type(list) == "table" then list else {}
end

local function save(tbl, list)
	host.set_raw(host.id(tbl), "Contents", host.encode(list))
end

local function public(entry)
	local values = {}
	for locale, text in entry.values or {} do
		values[locale] = text
	end
	return {
		Key = entry.key or "",
		Source = entry.source or "",
		Context = entry.context or "",
		Example = entry.examples or "",
		Values = values,
	}
end

local function stored(entry)
	if type(entry) ~= "table" then
		fail("LocalizationTable:SetEntries(): each entry must be a table")
	end
	local values = {}
	for locale, text in entry.Values or {} do
		values[str(locale)] = str(text)
	end
	return {
		key = entry.Key or "",
		source = entry.Source or "",
		context = entry.Context or "",
		examples = entry.Example or "",
		values = values,
	}
end

local function find(list, key, source, context)
	for i, entry in list do
		if (entry.key or "") == (key or "") and (entry.source or "") == (source or "") and (entry.context or "") == (context or "") then
			return i, entry
		end
	end
	return nil
end

-- Every table a translator looks in.
local function tables()
	local out = {}
	local service = game:GetService("LocalizationService")
	for _, child in service:GetDescendants() do
		if child:IsA("LocalizationTable") then
			table.insert(out, child)
		end
	end
	return out
end

local function valueFor(entry, locale)
	local values = entry.values or {}
	locale = string.lower(locale)
	for name, text in values do
		if string.lower(name) == locale then
			return text
		end
	end
	local language = string.match(locale, "^(%a+)")
	for name, text in values do
		if string.lower(name) == language or string.match(string.lower(name), "^(%a+)") == language then
			return text
		end
	end
	return nil
end

local translators = {} -- Translator id -> { locale, table }

local function newTranslator(locale, tbl)
	local translator, id = host.new("Translator", "Translator")
	host.set_raw(id, "LocaleId", locale)
	translators[id] = { locale = locale, table = tbl }
	return translator
end

local function lookIn(translator)
	local state = translators[host.id(translator)]
	if state.table then
		return { state.table }, state.locale
	end
	return tables(), state.locale
end

local function format(text, args)
	return (
		string.gsub(text, "{([^}:]+):?([^}]*)}", function(name, spec)
			local value
			if type(args) == "table" then
				value = args[tonumber(name)] or args[name]
			end
			if value == nil then
				return nil
			end
			if spec == "int" and type(value) == "number" then
				return tostring(math.round(value))
			elseif spec == "fixed" and type(value) == "number" then
				return string.format("%.2f", value)
			end
			return tostring(value)
		end)
	)
end

local TRANSLATOR = {}

function TRANSLATOR.Translate(self, context, text)
	if typeof(context) ~= "Instance" then
		fail("Unable to cast " .. typeName(context) .. " to Instance")
	end
	text = str(text)
	local list, locale = lookIn(self)
	for _, tbl in list do
		for _, entry in load(tbl) do
			if entry.source == text then
				local found = valueFor(entry, locale)
				if found then
					return found
				end
			end
		end
	end
	return text
end

function TRANSLATOR.FormatByKey(self, key, args)
	key = str(key)
	local list, locale = lookIn(self)
	for _, tbl in list do
		for _, entry in load(tbl) do
			if entry.key == key then
				return format(valueFor(entry, locale) or entry.source or "", args)
			end
		end
	end
	fail("Translator:FormatByKey(): no entry with key " .. key .. " for locale " .. locale)
end

local SERVICE = {}

function SERVICE.GetTranslatorForPlayerAsync(_, player)
	if not isPlayer(player) then
		fail("LocalizationService:GetTranslatorForPlayerAsync(): player must be a Player")
	end
	task.wait()
	local locale = player.LocaleId
	return newTranslator(if locale ~= "" then locale else "en-us", nil)
end
SERVICE.GetTranslatorForPlayer = function(_, player)
	if not isPlayer(player) then
		fail("LocalizationService:GetTranslatorForPlayer(): player must be a Player")
	end
	return newTranslator(if player.LocaleId ~= "" then player.LocaleId else "en-us", nil)
end

function SERVICE.GetTranslatorForLocaleAsync(_, locale)
	locale = str(locale)
	if locale == "" then
		fail("LocalizationService:GetTranslatorForLocaleAsync(): locale is empty")
	end
	task.wait()
	return newTranslator(locale, nil)
end

function SERVICE.GetCountryRegionForPlayerAsync(_, player)
	if not isPlayer(player) then
		fail("LocalizationService:GetCountryRegionForPlayerAsync(): player must be a Player")
	end
	task.wait()
	return "US"
end

function SERVICE.GetTableEntries(_, instance)
	local out = {}
	for _, tbl in tables() do
		if instance == nil or tbl == instance or tbl:IsDescendantOf(instance) then
			for _, entry in load(tbl) do
				table.insert(out, public(entry))
			end
		end
	end
	return out
end

function SERVICE.GetCorescriptLocalizations(_)
	return {}
end

local TABLE = {}

function TABLE.GetEntries(self)
	local out = {}
	for _, entry in load(self) do
		table.insert(out, public(entry))
	end
	return out
end

function TABLE.SetEntries(self, entries)
	if type(entries) ~= "table" then
		fail("Unable to cast " .. typeName(entries) .. " to Array")
	end
	local list = {}
	for _, entry in entries do
		table.insert(list, stored(entry))
	end
	save(self, list)
end

function TABLE.GetTranslator(self, locale)
	return newTranslator(str(locale), self)
end

-- `f(entry, list)` on the entry (key, source, context), saved after.
local function edit(method, f)
	return function(self, key, source, context, ...)
		local list = load(self)
		local i, entry = find(list, key, source, context)
		if not entry then
			if method == "SetEntryValue" then
				entry = { key = key or "", source = source or "", context = context or "", examples = "", values = {} }
				table.insert(list, entry)
				i = #list
			else
				fail("LocalizationTable:" .. method .. "(): no such entry")
			end
		end
		f(entry, list, i, ...)
		save(self, list)
	end
end

TABLE.SetEntryValue = edit("SetEntryValue", function(entry, _, _, locale, text)
	entry.values = entry.values or {}
	entry.values[str(locale)] = str(text)
end)
TABLE.RemoveEntryValue = edit("RemoveEntryValue", function(entry, _, _, locale)
	if entry.values then
		entry.values[str(locale)] = nil
	end
end)
TABLE.RemoveEntry = edit("RemoveEntry", function(_, list, i)
	table.remove(list, i)
end)
TABLE.SetEntryKey = edit("SetEntryKey", function(entry, _, _, value)
	entry.key = str(value)
end)
TABLE.SetEntrySource = edit("SetEntrySource", function(entry, _, _, value)
	entry.source = str(value)
end)
TABLE.SetEntryContext = edit("SetEntryContext", function(entry, _, _, value)
	entry.context = str(value)
end)
TABLE.SetEntryExample = edit("SetEntryExample", function(entry, _, _, value)
	entry.examples = str(value)
end)

function TABLE.RemoveTargetLocale(self, locale)
	locale = str(locale)
	local list = load(self)
	for _, entry in list do
		if entry.values then
			entry.values[locale] = nil
		end
	end
	save(self, list)
end

local TABLES = { LocalizationService = SERVICE, LocalizationTable = TABLE, Translator = TRANSLATOR }

function M.member(class, _, key)
	local member = TABLES[class][key]
	return member, member ~= nil
end

return M
