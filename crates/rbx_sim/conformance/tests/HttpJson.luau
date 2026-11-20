-- HttpService's offline half on a server: JSONEncode's exact text (number
-- formatting, escapes, sparse and mixed tables, key order, values JSON
-- can't hold), JSONDecode (numbers, nulls, scalars, its errors),
-- GenerateGUID and UrlEncode, and the errors a malformed request gives
-- before anything is sent. Nothing here reaches the network.
return function(h)
	local HttpService = game:GetService("HttpService")
	local result = {}
	local function err(f)
		local ok, message = pcall(f)
		return ok and "ok" or (tostring(message):gsub("^.-:%d+: ", ""))
	end
	local function enc(v)
		local ok, text = pcall(HttpService.JSONEncode, HttpService, v)
		return ok and text or "error: " .. (tostring(text):gsub("^.-:%d+: ", ""))
	end
	local numbers = {
		zero = 0,
		negzero = -0,
		int = 42,
		neg = -7,
		frac = 0.1,
		third = 1 / 3,
		twothirds = 2 / 3,
		half = 0.5,
		big = 1e15,
		bigger = 1e16,
		huge = 1e20,
		e300 = 1e300,
		small = 1e-5,
		tiny = 1e-7,
		pi = math.pi,
		maxint = 2 ^ 53,
		maxint1 = 2 ^ 53 + 2,
		i31 = 2 ^ 31,
		i32 = 2 ^ 32 + 0.5,
		cents = 19.99,
		third_e10 = 1e10 / 3,
	}
	for name, n in numbers do
		result["num_" .. name] = enc(n)
	end
	result.nan = enc(0 / 0)
	result.inf = enc(math.huge)
	result.neginf = enc(-math.huge)
	result.nan_in_table = enc({ 0 / 0 })
	result.str_escapes = enc("q\"b\\s/n\nt\tr\rb\bf\f")
	result.str_control = enc("\0\1\31\127")
	result.str_unicode = enc("é✓\u{1F600}")
	result.str_bad_utf8 = enc("\255")
	result.str_html = enc("<a href='x'>&</a>")
	result.str_u2028 = enc("\u{2028}\u{2029}")
	result.empty = enc({})
	result.array = enc({ 1, "two", true, false })
	result.sparse = enc({ 1, nil, 3 })
	result.sparse_far = enc({ [1] = 1, [5] = 5 })
	result.only_far = enc({ [3] = 3 })
	result.zero_key = enc({ [0] = "z", [1] = "a" })
	result.float_key = enc({ [1.5] = "x" })
	result.mixed = enc({ 1, 2, x = 3 })
	result.bool_key = enc({ [true] = 1 })
	result.nested = enc({ a = { b = { c = {} } } })
	result.one_key = enc({ k = "v" })
	result.nil_value = enc(nil)
	result.top_string = enc("s")
	result.top_number = enc(3)
	result.top_bool = enc(true)
	result.vector3 = enc(Vector3.new(1, 2, 3))
	result.vector3_in_table = enc({ v = Vector3.new(1, 2, 3) })
	result.color3 = enc(Color3.new(1, 0, 0))
	result.cframe = enc(CFrame.new())
	result.udim2 = enc(UDim2.new(1, 2, 3, 4))
	result.instance = enc(workspace)
	result.enum = enc(Enum.Material.Plastic)
	result.func = enc(print)
	result.func_in_table = enc({ f = print })
	result.cycle = (function()
		local t = {}
		t.t = t
		return enc(t)
	end)()
	local deep = {}
	local cursor = deep
	for _ = 1, 600 do
		cursor.d = {}
		cursor = cursor.d
	end
	result.deep = (enc(deep):sub(1, 60))
	-- Key order: sorted, insertion, or hash order?
	local keyed = {}
	for _, name in { "zeta", "alpha", "mid", "beta", "omega", "a", "b", "c", "d10", "d2" } do
		keyed[name] = 1
	end
	result.key_order = enc(keyed)
	local numeric = {}
	for _, n in { 3, 1, 2 } do
		numeric[tostring(n)] = n
	end
	result.string_number_keys = enc(numeric)

	local function dec(text)
		local ok, v = pcall(HttpService.JSONDecode, HttpService, text)
		if not ok then
			return "error: " .. (tostring(v):gsub("^.-:%d+: ", ""))
		end
		if type(v) == "table" then
			local parts = {}
			local keys = {}
			for k in v do
				table.insert(keys, k)
			end
			table.sort(keys, function(a, b)
				return tostring(a) < tostring(b)
			end)
			for _, k in keys do
				table.insert(parts, type(k):sub(1, 1) .. tostring(k) .. "=" .. type(v[k]) .. ":" .. tostring(v[k]))
			end
			return "{" .. table.concat(parts, ",") .. "} #" .. #v
		end
		return type(v) .. ":" .. (type(v) == "number" and string.format("%.17g", v) or tostring(v))
	end
	result.dec_int = dec("42")
	result.dec_float = dec("0.1")
	result.dec_exp = dec("1e400")
	result.dec_negexp = dec("-1e-400")
	result.dec_bigint = dec("12345678901234567890")
	result.dec_string = dec('"x\\u00e9\\ud83d\\ude00"')
	result.dec_true = dec("true")
	result.dec_null = dec("null")
	result.dec_array_nulls = dec("[1,null,3]")
	result.dec_trailing_null = dec("[1,2,null]")
	result.dec_object_null = dec('{"a":null,"b":1}')
	result.dec_dup = dec('{"a":1,"a":2}')
	result.dec_numkey = dec('{"1":"a","2":"b"}')
	result.dec_empty_object = dec("{}")
	result.dec_empty_array = dec("[]")
	result.dec_bad = dec("{bad}")
	result.dec_trailing_comma = dec("[1,2,]")
	result.dec_single_quotes = dec("{'a':1}")
	result.dec_empty = dec("")
	result.dec_spaces = dec("  [ 1 , 2 ]  ")
	result.dec_garbage_after = dec("[1] x")
	result.dec_nan = dec("NaN")
	result.dec_comment = dec("[1 /* c */]")
	result.dec_number_arg = err(function()
		HttpService:JSONDecode(5)
	end)
	result.dec_nil_arg = err(function()
		HttpService:JSONDecode(nil)
	end)
	result.roundtrip_frac = string.format("%.17g", HttpService:JSONDecode(HttpService:JSONEncode(0.1)))
	result.roundtrip_third = string.format("%.17g", HttpService:JSONDecode(HttpService:JSONEncode(1 / 3)))

	local braced = HttpService:GenerateGUID()
	result.guid_braced = (braced:gsub("%x", "x"))
	result.guid_bare = (HttpService:GenerateGUID(false):gsub("%x", "x"))
	result.guid_case = braced == braced:upper() and "upper" or "lower"
	result.guid_version = braced:sub(16, 16)
	result.guid_variant = string.find("89AB", braced:sub(21, 21), 1, true) ~= nil
	result.url_encode = HttpService:UrlEncode("a b&c=d/é~-_.!*'()?#+%")
	result.url_encode_empty = HttpService:UrlEncode("")
	result.url_encode_number = err(function()
		return HttpService:UrlEncode(5)
	end)

	result.get_not_url = err(function()
		HttpService:GetAsync("not a url")
	end)
	result.request_no_url = err(function()
		HttpService:RequestAsync({})
	end)
	result.request_not_table = err(function()
		HttpService:RequestAsync("x")
	end)
	result.request_bad_method = err(function()
		HttpService:RequestAsync({ Url = "not a url", Method = "FETCH" })
	end)
	result.request_body_on_get = err(function()
		HttpService:RequestAsync({ Url = "not a url", Method = "GET", Body = "x" })
	end)
	result.post_no_data = err(function()
		HttpService:PostAsync("not a url")
	end)
	result.http_enabled = err(function()
		return HttpService.HttpEnabled
	end)
	return result
end
