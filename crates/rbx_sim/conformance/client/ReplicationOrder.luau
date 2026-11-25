-- ReplicationOrder on the client.
local s
return function(ch, step)
	local ReplicatedStorage = game:GetService("ReplicatedStorage")
	local Players = game:GetService("Players")
	if step == "setup" then
		s = { log = {}, conns = {} }
		local function log(text)
			table.insert(s.log, text)
		end
		local function names(list)
			local out = {}
			for _, i in list do
				table.insert(out, i.Name)
			end
			table.sort(out)
			return table.concat(out, "+")
		end
		s.conns[1] = workspace.ChildAdded:Connect(function(c)
			if c.Name:sub(1, 5) ~= "Batch" then
				return
			end
			local part = c:FindFirstChild("P")
			log(
				string.format(
					"added:%s:%s:a=%s:pt=%s",
					c.Name,
					names(c:GetChildren()),
					tostring(c:GetAttribute("a")),
					part and tostring(part.Transparency) or "nil"
				)
			)
			c.ChildAdded:Connect(function(x)
				log("child+:" .. x.Name)
			end)
			c.ChildRemoved:Connect(function(x)
				log("child-:" .. x.Name)
			end)
			c:GetPropertyChangedSignal("Name"):Connect(function()
				log("name:" .. c.Name)
			end)
			c.AttributeChanged:Connect(function(n)
				log("attr:" .. tostring(c:GetAttribute(n)))
			end)
			if part then
				part:GetPropertyChangedSignal("Transparency"):Connect(function()
					log("pt:" .. part.Transparency)
				end)
			end
		end)
		s.conns[2] = ReplicatedStorage.ChildAdded:Connect(function(c)
			if c.Name == "Hop" then
				log("rs+:Hop" .. (s.hop and (rawequal(s.hop, c) and ":same" or ":new") or ""))
				s.hop = s.hop or c
			end
		end)
		s.conns[3] = ReplicatedStorage.ChildRemoved:Connect(function(c)
			if c.Name == "Hop" then
				log("rs-:Hop:parent=" .. tostring(c.Parent))
			end
		end)
		ReplicatedStorage:WaitForChild("ClientDestroys"):Destroy()
		local mine = Instance.new("Part")
		mine.Name = "ClientMade"
		mine.Anchored = true
		mine.Parent = workspace
		workspace.Baseplate.Transparency = 0.7
		Players.LocalPlayer:SetAttribute("FromClient", 1)
		return true
	end
	for _, c in s.conns do
		c:Disconnect()
	end
	local r = {}
	r.log = table.concat(s.log, " ")
	r.server_storage_children = #game:GetService("ServerStorage"):GetChildren()
	r.server_script_service_children = #game:GetService("ServerScriptService"):GetChildren()
	r.player_scripts = Players.LocalPlayer:FindFirstChild("PlayerScripts") ~= nil
	r.backpack = Players.LocalPlayer:FindFirstChild("Backpack") ~= nil
	r.player_gui = Players.LocalPlayer:FindFirstChild("PlayerGui") ~= nil
	r.is_loaded = game:IsLoaded()
	r.rf_loaded_at_start = _G.rf_loaded_at_start == true
	r.rf_baseplate_at_start = _G.rf_baseplate_at_start == true
	r.rf_baseplate_after_loaded = _G.rf_baseplate_after_loaded == true
	r.ps_loaded_at_start = _G.ps_loaded_at_start == true
	r.rc_client_ws = _G.rc_client_ws == true
	r.rc_client_rs = _G.rc_client_rs == true
	r.local_ws = _G.local_ws == true
	r.legacy_rs = _G.legacy_rs == true
	r.rc_server_rs = _G.rc_server_rs == true
	r.client_baseplate_transparency = workspace.Baseplate.Transparency
	workspace.Baseplate.Transparency = 0
	local mine = workspace:FindFirstChild("ClientMade")
	if mine then
		mine:Destroy()
	end
	return r
end
