-- LocalizationTable entries and a Translator over them, on a server: a
-- table filled with SetEntries, an edit, lookups by key and source, a
-- regional locale falling back to its language, formatting, misses.
-- Needs a player.
return function(h)
	local LocalizationService = game:GetService("LocalizationService")
	local Players = game:GetService("Players")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local player = Players:GetPlayers()[1]
	while not player do
		Players.PlayerAdded:Wait()
		player = Players:GetPlayers()[1]
	end
	local tbl = Instance.new("LocalizationTable")
	tbl.Name = "ConformanceTable"
	tbl:SetEntries({
		{ Key = "Greeting", Source = "Hello", Context = "", Example = "", Values = { fr = "Bonjour", es = "Hola" } },
		{ Key = "Coins", Source = "{1} coins", Context = "", Example = "", Values = { fr = "{1} pièces" } },
		{ Key = "Named", Source = "Hi {name}", Context = "", Example = "", Values = { fr = "Salut {name}" } },
	})
	tbl.Parent = LocalizationService
	result.entries = #tbl:GetEntries()
	tbl:SetEntryValue("Greeting", "Hello", "", "de", "Hallo")
	result.entry_values = (function()
		local locales = {}
		for locale in tbl:GetEntries()[1].Values do
			table.insert(locales, locale)
		end
		table.sort(locales)
		return table.concat(locales, ",")
	end)()
	local fr = LocalizationService:GetTranslatorForLocaleAsync("fr-fr")
	result.locale = fr.LocaleId
	result.by_key = fr:FormatByKey("Greeting")
	result.by_key_args = fr:FormatByKey("Coins", { 5 })
	result.by_key_named = fr:FormatByKey("Named", { name = "Ana" })
	result.translate = fr:Translate(game, "Hello")
	result.translate_miss = fr:Translate(game, "Nothing here")
	result.key_miss = err(function()
		return fr:FormatByKey("Missing")
	end)
	local de = tbl:GetTranslator("de")
	result.table_translator = de:FormatByKey("Greeting")
	local ja = LocalizationService:GetTranslatorForLocaleAsync("ja")
	result.no_value_falls_back = ja:FormatByKey("Greeting")
	result.player_translator = LocalizationService:GetTranslatorForPlayerAsync(player).ClassName
	result.translate_bad_context = err(function()
		return fr:Translate(nil, "Hello")
	end)
	tbl:RemoveEntry("Coins", "{1} coins", "")
	result.after_remove = #tbl:GetEntries()
	tbl:Destroy()
	return result
end
