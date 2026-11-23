--!nonstrict
-- The avatar API a Local Run answers in Luau: Humanoid's accessories,
-- body parts and descriptions, HumanoidDescription's emotes and
-- accessories, the avatar members of Player and Players (bans as Studio
-- Play keeps them: for this run only), Teams, AvatarCreationService and
-- AvatarEditorService's lookups. Our own, from the creator docs and the
-- AvatarApi golden; members not here fall through to the engine's own.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function instanceOf(value, class)
	return typeof(value) == "Instance" and value:IsA(class)
end

local function integer(value, what)
	if type(value) == "string" and tonumber(value) then
		value = tonumber(value)
	end
	if type(value) ~= "number" then
		fail("Unable to cast " .. (value == nil and "nil" or typeof(value)) .. " to " .. (what or "int64"))
	end
	return value
end

local function signals(t, id, key)
	if t[key] then
		return host.signal(id, key), true
	end
end

-- Humanoid ------------------------------------------------------------------

local HUMANOID = {}
local HUMANOID_SIGNALS = { Touched = true, Strafing = true, ApplyDescriptionFinished = true }

local function accessories(model)
	local out = {}
	if model then
		for _, child in model:GetChildren() do
			if child:IsA("Accoutrement") then
				table.insert(out, child)
			end
		end
	end
	return out
end

function HUMANOID.GetAccessories(self)
	return accessories(self.Parent)
end

-- The attachment on the character an accessory's handle attachment names,
-- outside any accessory.
local function matching(model, name)
	for _, d in model:GetDescendants() do
		if d:IsA("Attachment") and d.Name == name and not d:FindFirstAncestorWhichIsA("Accoutrement") then
			return d
		end
	end
end

local function weld(parent, name, part0, part1, c0, c1)
	local w = Instance.new("Weld")
	w.Name = name
	w.C0 = c0
	w.C1 = c1
	w.Part0 = part0
	w.Part1 = part1
	w.Parent = parent
	return w
end

function HUMANOID.AddAccessory(self, accessory)
	if not instanceOf(accessory, "Accoutrement") then
		fail("AddAccessory should be passed a valid Accessory object.")
	end
	local model = self.Parent
	accessory.Parent = model
	local handle = accessory:FindFirstChild("Handle")
	if not (model and handle and handle:IsA("BasePart")) then
		return
	end
	for _, attachment in handle:GetChildren() do
		if attachment:IsA("Attachment") then
			local target = matching(model, attachment.Name)
			if target then
				-- A live server moves the handle onto the attachment, holds
				-- it with a RigidConstraint and notes the handle's size
				-- (AvatarApi `acc_new`, `acc_handle_pos`).
				handle.CFrame = target.WorldCFrame * attachment.CFrame:Inverse()
				local size = Instance.new("Vector3Value")
				size.Name = "OriginalSize"
				size.Value = handle.Size
				size.Parent = handle
				local rigid = Instance.new("RigidConstraint")
				rigid.Name = "AccessoryRigidConstraint"
				rigid.Attachment0 = attachment
				rigid.Attachment1 = target
				rigid.Parent = handle
				return
			end
		end
	end
	-- No matching attachment (or none at all): the head holds it by
	-- AttachmentPoint, with a "HeadWeld" in the head (AvatarApi
	-- `acc_add_legacy`, `acc_add_unmatched`).
	local head = model:FindFirstChild("Head")
	if head and head:IsA("BasePart") then
		local c0 = CFrame.new(0, 0.5, 0)
		handle.CFrame = head.CFrame * c0 * accessory.AttachmentPoint:Inverse()
		weld(head, "HeadWeld", head, handle, c0, accessory.AttachmentPoint)
	end
end

function HUMANOID.RemoveAccessories(self)
	for _, accessory in accessories(self.Parent) do
		accessory:Destroy()
	end
end

local R15_PARTS = {
	"Head",
	"UpperTorso",
	"LowerTorso",
	"LeftUpperArm",
	"LeftLowerArm",
	"LeftHand",
	"RightUpperArm",
	"RightLowerArm",
	"RightHand",
	"LeftUpperLeg",
	"LeftLowerLeg",
	"LeftFoot",
	"RightUpperLeg",
	"RightLowerLeg",
	"RightFoot",
}
local R15_PART = {}
for _, name in R15_PARTS do
	R15_PART[name] = name
end
R15_PART.HumanoidRootPart = "RootPart"

local LIMBS = {
	Head = "Head",
	Torso = "Torso",
	UpperTorso = "Torso",
	LowerTorso = "Torso",
	["Left Arm"] = "LeftArm",
	LeftUpperArm = "LeftArm",
	LeftLowerArm = "LeftArm",
	LeftHand = "LeftArm",
	["Right Arm"] = "RightArm",
	RightUpperArm = "RightArm",
	RightLowerArm = "RightArm",
	RightHand = "RightArm",
	["Left Leg"] = "LeftLeg",
	LeftUpperLeg = "LeftLeg",
	LeftLowerLeg = "LeftLeg",
	LeftFoot = "LeftLeg",
	["Right Leg"] = "RightLeg",
	RightUpperLeg = "RightLeg",
	RightLowerLeg = "RightLeg",
	RightFoot = "RightLeg",
}

function HUMANOID.GetBodyPartR15(self, part)
	if typeof(part) ~= "Instance" or part.Parent ~= self.Parent then
		fail("Part does not have the same parent as Humanoid")
	end
	return Enum.BodyPartR15[R15_PART[part.Name] or "Unknown"]
end

function HUMANOID.GetLimb(_, part)
	if typeof(part) ~= "Instance" then
		fail("GetLimb must be passed a non-nil part.")
	end
	return Enum.Limb[LIMBS[part.Name] or "Unknown"]
end

-- The new part joins the rig where it stands, keeping its name; the old
-- part goes, and the joints in it with it (AvatarApi `replace`).
function HUMANOID.ReplaceBodyPartR15(self, bodyPart, part)
	if typeof(bodyPart) ~= "EnumItem" then
		fail("Argument 1 missing or nil")
	end
	if not instanceOf(part, "BasePart") then
		fail("Argument 2 missing or nil")
	end
	if bodyPart == Enum.BodyPartR15.RootPart then
		fail("Humanoid has no HumanoidRootPart.")
	end
	local model = self.Parent
	local old = model and model:FindFirstChild(bodyPart.Name)
	if bodyPart == Enum.BodyPartR15.Unknown or not (old and old:IsA("BasePart")) then
		return false
	end
	part.Parent = model
	old:Destroy()
	return true
end

-- Motor6Ds between parts whose RigAttachments share a name, from the root
-- outwards, each in its child part.
function HUMANOID.BuildRigFromAttachments(self)
	local model = self.Parent
	local root = model and model:FindFirstChild("HumanoidRootPart")
	if not (root and root:IsA("BasePart")) then
		return
	end
	local parts = {}
	for _, child in model:GetChildren() do
		if child:IsA("BasePart") then
			table.insert(parts, child)
		end
	end
	local done = { [root] = true }
	local queue = { root }
	while #queue > 0 do
		local parent = table.remove(queue, 1)
		for _, a in parent:GetChildren() do
			if a:IsA("Attachment") and string.match(a.Name, "RigAttachment$") then
				local joint = string.gsub(a.Name, "RigAttachment$", "")
				for _, part in parts do
					local b = not done[part] and part:FindFirstChild(a.Name)
					if b and b:IsA("Attachment") then
						done[part] = true
						local old = part:FindFirstChild(joint)
						if old and old:IsA("Motor6D") then
							old:Destroy()
						end
						local motor = Instance.new("Motor6D")
						motor.Name = joint
						motor.Part0 = parent
						motor.Part1 = part
						motor.C0 = a.CFrame
						motor.C1 = b.CFrame
						motor.Parent = part
						table.insert(queue, part)
					end
				end
			end
		end
	end
end

local function description(value)
	if value == nil then
		fail("Argument 1 missing or nil")
	end
	if not instanceOf(value, "HumanoidDescription") then
		fail("Argument 1 is invalid: expected 'HumanoidDescription' instance type")
	end
	return value
end

local function applyAsync(self, value)
	description(value)
	self:ApplyDescription(value)
	local applied = self:FindFirstChildOfClass("HumanoidDescription")
	host.fire(host.id(self), "ApplyDescriptionFinished", applied or value)
end

HUMANOID.ApplyDescriptionAsync = applyAsync
HUMANOID.ApplyDescriptionResetAsync = applyAsync

function HUMANOID.ApplyAvatarRulesAsync(_, _rules) end

local emotesOf

function HUMANOID.PlayEmoteAsync(self, name)
	if type(name) ~= "string" then
		fail("Unable to cast " .. typeof(name) .. " to string")
	end
	local applied = self:FindFirstChildOfClass("HumanoidDescription")
	local ids = applied and emotesOf(applied)[name]
	if not ids or not ids[1] then
		fail("No emote exists with name " .. name)
	end
	local animate = self.Parent and self.Parent:FindFirstChild("Animate")
	local play = animate and animate:FindFirstChild("PlayEmote")
	if not (play and play:IsA("BindableFunction")) then
		return false
	end
	local animation = Instance.new("Animation")
	animation.AnimationId = "rbxassetid://" .. ids[1]
	local ok, played = pcall(play.Invoke, play, animation)
	return ok and played == true
end

-- HumanoidDescription -------------------------------------------------------

local DESCRIPTION = {}
local DESCRIPTION_SIGNALS = { EmotesChanged = true, EquippedEmotesChanged = true }
-- Kept as raw properties so a copy (GetAppliedDescription, Clone) keeps them.
local EMOTES, EQUIPPED, BLOB = "EmotesDataInternal", "EquippedEmotesDataInternal", "AccessoryBlob"

local function rawTable(self, name)
	local text = host.raw(host.id(self), name)
	local ok, value = pcall(host.decode, text or "")
	return ok and type(value) == "table" and value or {}
end

local function setRaw(self, name, value)
	host.set_raw(host.id(self), name, host.encode(value))
end

local function copyEmotes(emotes)
	local out = {}
	for name, ids in emotes do
		out[name] = table.clone(ids)
	end
	return out
end

function emotesOf(self)
	local out = {}
	for name, ids in rawTable(self, EMOTES) do
		local list = {}
		for _, id in ids do
			table.insert(list, tonumber(id))
		end
		table.sort(list)
		out[name] = list
	end
	return out
end

local function setEmotes(self, emotes)
	setRaw(self, EMOTES, emotes)
	host.fire(host.id(self), "EmotesChanged", copyEmotes(emotesOf(self)))
end

function DESCRIPTION.GetEmotes(self)
	return emotesOf(self)
end

function DESCRIPTION.AddEmote(self, name, id)
	if type(name) ~= "string" then
		fail("Unable to cast " .. typeof(name) .. " to string")
	end
	id = integer(id)
	local emotes = emotesOf(self)
	emotes[name] = emotes[name] or {}
	table.insert(emotes[name], id)
	setEmotes(self, emotes)
end

function DESCRIPTION.RemoveEmote(self, name)
	local emotes = emotesOf(self)
	emotes[name] = nil
	setEmotes(self, emotes)
end

-- Entries that aren't a name to a list of ids are dropped.
function DESCRIPTION.SetEmotes(self, emotes)
	if type(emotes) ~= "table" then
		fail("Unable to cast " .. typeof(emotes) .. " to Dictionary")
	end
	local out = {}
	for name, ids in emotes do
		if type(name) == "string" and type(ids) == "table" then
			local list = {}
			for _, id in ids do
				if tonumber(id) then
					table.insert(list, tonumber(id))
				end
			end
			out[name] = list
		end
	end
	setEmotes(self, out)
end

local function equippedOf(self)
	local out = {}
	for _, e in rawTable(self, EQUIPPED) do
		table.insert(out, { Slot = e.Slot, Name = e.Name })
	end
	return out
end

function DESCRIPTION.GetEquippedEmotes(self)
	return equippedOf(self)
end

-- Names (slots in order) or `{ Slot, Name }` entries; names aren't checked
-- against the emotes.
function DESCRIPTION.SetEquippedEmotes(self, equipped)
	if type(equipped) ~= "table" then
		fail("Unable to cast " .. typeof(equipped) .. " to Array")
	end
	local out = {}
	for i, e in ipairs(equipped) do
		if type(e) == "string" then
			table.insert(out, { Slot = i, Name = e })
		elseif type(e) == "table" and type(e.Name) == "string" then
			table.insert(out, { Slot = tonumber(e.Slot) or i, Name = e.Name })
		end
	end
	table.sort(out, function(a, b)
		return a.Slot < b.Slot
	end)
	setRaw(self, EQUIPPED, out)
	host.fire(host.id(self), "EquippedEmotesChanged", equippedOf(self))
end

-- Rigid accessory types and the property each is kept in, in the order
-- GetAccessories lists them.
local RIGID = {
	{ "Hat", "HatAccessory" },
	{ "Hair", "HairAccessory" },
	{ "Face", "FaceAccessory" },
	{ "Neck", "NeckAccessory" },
	{ "Shoulder", "ShouldersAccessory" },
	{ "Front", "FrontAccessory" },
	{ "Back", "BackAccessory" },
	{ "Waist", "WaistAccessory" },
}
local RIGID_PROPERTY = {}
for _, r in RIGID do
	RIGID_PROPERTY[r[1]] = r[2]
end

local function entry(kind, id, layered)
	return {
		AssetId = id,
		AccessoryType = Enum.AccessoryType[kind],
		IsLayered = layered,
		Position = Vector3.zero,
		Rotation = Vector3.zero,
		Scale = Vector3.one,
	}
end

function DESCRIPTION.GetAccessories(self, includeRigid)
	local out = {}
	if includeRigid then
		for _, r in RIGID do
			local ids = {}
			for id in string.gmatch(self[r[2]], "%d+") do
				table.insert(ids, 1, tonumber(id))
			end
			for _, id in ids do
				table.insert(out, entry(r[1], id, false))
			end
		end
	end
	for _, l in rawTable(self, BLOB) do
		local e = entry(l.AccessoryType, l.AssetId, true)
		e.Order = l.Order
		e.Puffiness = l.Puffiness or 1
		table.insert(out, e)
	end
	return out
end

function DESCRIPTION.SetAccessories(self, list, includeRigid)
	if type(list) ~= "table" then
		fail("Unable to cast " .. typeof(list) .. " to Array")
	end
	local rigid, layered = {}, {}
	for _, a in ipairs(list) do
		local kind = typeof(a) == "table" and typeof(a.AccessoryType) == "EnumItem" and a.AccessoryType.Name
		local id = kind and tonumber(a.AssetId)
		if id and a.IsLayered then
			table.insert(layered, { AssetId = id, AccessoryType = kind, Order = a.Order, Puffiness = a.Puffiness })
		elseif id and includeRigid and RIGID_PROPERTY[kind] then
			local ids = rigid[kind] or {}
			table.insert(ids, tostring(id))
			rigid[kind] = ids
		end
	end
	if includeRigid then
		for _, r in RIGID do
			self[r[2]] = table.concat(rigid[r[1]] or {}, ",")
		end
	end
	setRaw(self, BLOB, layered)
end

local ACCESSORY_DESCRIPTION = {}

function ACCESSORY_DESCRIPTION.GetAppliedInstance(self)
	local parent = self.Parent
	if not instanceOf(parent, "HumanoidDescription") then
		fail("AccessoryDescription is not parented to a HumanoidDescription. Did you call ApplyDescription()?")
	end
	local humanoid = parent.Parent
	if not instanceOf(humanoid, "Humanoid") then
		return nil
	end
	for _, a in accessories(humanoid.Parent) do
		if a:GetAttribute("AssetId") == self.AssetId then
			return a
		end
	end
	return nil
end

-- Player and Players --------------------------------------------------------

local PLAYER = {}
local PLAYER_SIGNALS = { Idled = true, OnTeleport = true, EmotesLoaded = true }

function PLAYER.GetCameraState(_)
	fail("GetCameraState requires Server Authority to be enabled")
end

function PLAYER.ClearCachedAvatarAppearance(_) end

function PLAYER.PromptAgeCheck(_) end

local APPEARANCE = { "Accoutrement", "Shirt", "Pants", "ShirtGraphic", "BodyColors", "CharacterMesh" }

function PLAYER.ClearCharacterAppearance(self)
	local character = self.Character
	if not character then
		return
	end
	for _, child in character:GetChildren() do
		for _, class in APPEARANCE do
			if child:IsA(class) then
				child:Destroy()
				break
			end
		end
	end
end

function PLAYER.LoadCharacterWithHumanoidDescriptionAsync(self, value)
	description(value)
	self:LoadCharacterAsync()
	local humanoid = self.Character and self.Character:FindFirstChildOfClass("Humanoid")
	if humanoid then
		humanoid:ApplyDescription(value)
	end
end

function PLAYER.LoadCharacterWithAvatarRulesAsync(self, rules)
	if rules == nil then
		fail("Argument 1 missing or nil")
	end
	self:LoadCharacterAsync()
end

local PLAYERS = {}
local PLAYERS_SIGNALS = { PlayerMembershipChanged = true, UserSubscriptionStatusChanged = true }

-- The bans of this run, newest last: Studio tests them without touching
-- the live game's. A player joining while banned is kicked too.
local bans = {}
local watching = false

local function banned(id)
	for i = #bans, 1, -1 do
		if bans[i].UserId == id then
			return bans[i].Ban and bans[i] or nil
		end
	end
	return nil
end

local function userIds(config)
	if type(config) ~= "table" then
		fail("Unable to cast " .. typeof(config) .. " to Dictionary")
	end
	local ids = config.UserIds
	if type(ids) ~= "table" or (next(ids) ~= nil and ids[1] == nil) then
		fail("Config must contain field UserIds with type array")
	end
	if #ids > 50 then
		fail("UserIds exceeded the maximum length of 50")
	end
	for _, id in ids do
		integer(id, "User")
	end
	return ids
end

local function text(config, key, limit)
	local value = config[key]
	if value ~= nil and type(value) ~= "string" then
		fail("Config field " .. key .. " must be a string")
	end
	if value and #value > limit then
		fail(key .. " exceeded the maximum length of " .. limit)
	end
	return value or ""
end

local function serverOnly(what)
	if not host.server() then
		fail(what .. " can only be called from the server")
	end
end

function PLAYERS.BanAsync(self, config)
	serverOnly("BanAsync")
	local ids = userIds(config)
	local duration = config.Duration
	if type(duration) ~= "number" or (duration <= 0 and duration ~= -1) then
		fail("Duration must be a positive number or -1")
	end
	local display = text(config, "DisplayReason", 400)
	local private = text(config, "PrivateReason", 1000)
	if not watching then
		watching = true
		self.PlayerAdded:Connect(function(player)
			local ban = banned(player.UserId)
			if ban then
				player:Kick(ban.DisplayReason)
			end
		end)
	end
	for _, id in ids do
		table.insert(bans, {
			Ban = true,
			UserId = id,
			Duration = duration,
			DisplayReason = display,
			PrivateReason = private,
			StartTime = DateTime.now():ToIsoDate(),
			PlaceId = config.ApplyToUniverse == false and game.PlaceId or nil,
		})
		local player = self:GetPlayerByUserId(id)
		if player then
			player:Kick(display)
		end
	end
end

function PLAYERS.UnbanAsync(_, config)
	serverOnly("UnbanAsync")
	for _, id in userIds(config) do
		table.insert(bans, {
			Ban = false,
			UserId = id,
			Duration = 0,
			DisplayReason = "",
			PrivateReason = "",
			StartTime = DateTime.now():ToIsoDate(),
		})
	end
end

local BAN_PAGES = {}

function PLAYERS.GetBanHistoryAsync(_, id)
	serverOnly("GetBanHistoryAsync")
	id = integer(id, "User")
	local page = {}
	for _, ban in bans do
		if ban.UserId == id then
			local copy = table.clone(ban)
			copy.UserId = nil
			table.insert(page, copy)
		end
	end
	local pages, pagesId = host.new("BanHistoryPages", "BanHistoryPages")
	BAN_PAGES[pagesId] = page
	return pages
end

local BAN_HISTORY = {}

function BAN_HISTORY.GetCurrentPage(self)
	return BAN_PAGES[host.id(self)] or {}
end

function BAN_HISTORY.AdvanceToNextPageAsync(_)
	fail("No pages to advance to")
end

function PLAYERS.GetHumanoidDescriptionFromOutfitIdAsync(self, id)
	id = integer(id)
	if id <= 0 then
		fail("Players:GetHumanoidDescriptionFromOutfitId() got negative outfitId")
	end
	task.wait()
	return self["GetHumanoidDescriptionFromOutfitId#fetch"](self, id)
end

function PLAYERS.GetProfileConfigurationFromUserIdAsync(_, id)
	serverOnly("GetProfileConfigurationFromUserIdAsync")
	integer(id, "User")
	return {}
end

-- Teams, AvatarCreationService, AvatarEditorService --------------------------

local TEAMS = {}

function TEAMS.GetTeams(self)
	local out = {}
	for _, child in self:GetChildren() do
		if child:IsA("Team") then
			table.insert(out, child)
		end
	end
	return out
end

local CREATION = {}

function CREATION.GetValidationRules(_)
	return {
		AccessoryRules = {},
		BodyPartRules = {},
		FullBodyRules = {},
		MakeupRules = {},
		MeshRules = {},
		TextureRules = {},
	}
end

-- The rest needs Roblox's avatar generation and creation services, which a
-- Local Run has no access to: past the checks a live server makes first
-- (AvatarApi `acs_*`), each says so.
local function needed(value)
	if value == nil then
		fail("Argument 1 missing or nil")
	end
end

local function offline(name)
	fail("AvatarCreationService:" .. name .. " is not available in a Local Run")
end

for _, name in {
	"AutoSetupAvatarAsync",
	"GenerateAvatar2DPreviewAsync",
	"GenerateAvatarAsync",
	"PromptCreateAvatarAssetAsync",
	"PromptCreateAvatarAsync",
	"PromptCreateMakeupAsync",
	"PromptSelectAvatarGenerationImageAsync",
	"RequestAvatarGenerationSessionAsync",
	"ValidateUGCAccessoryAsync",
	"ValidateUGCBodyPartAsync",
	"ValidateUGCFullBodyAsync",
} do
	CREATION[name] = function(_, first)
		needed(first)
		offline(name)
	end
end

function CREATION.PrepareAvatarForPreviewAsync(_, model)
	needed(model)
	if typeof(model) ~= "Instance" or not model:IsA("Model") then
		fail("Unable to cast " .. typeof(model) .. " to Model")
	end
end

function CREATION.GetBatchTokenDetailsAsync(_, tokenIds)
	if type(tokenIds) ~= "table" then
		fail("Unable to cast " .. typeof(tokenIds) .. " to Array")
	end
	if #tokenIds == 0 then
		fail("AvatarCreationService:getTokenDetailsAsync - tokenIds must not be empty")
	end
	offline("GetBatchTokenDetailsAsync")
end

function CREATION.LoadAvatar2DPreviewAsync(_)
	if host.server() then
		fail("AvatarCreationService:LoadAvatarPreviewImageAsync should only be called on the client.")
	end
	offline("LoadAvatar2DPreviewAsync")
end

function CREATION.LoadGeneratedAvatarAsync(_, id)
	if id == nil or tostring(id) == "" then
		fail("AvatarCreationService:LoadAvatarModelAsync expects Id to be non empty.")
	end
	offline("LoadGeneratedAvatarAsync")
end

local CREATION_SIGNALS = {
	AvatarAssetModerationCompleted = true,
	AvatarModerationCompleted = true,
	AvatarOutfitModerationCompleted = true,
}

local EDITOR = {}

local function clientOnly()
	if host.server() then
		fail("This method can only be called on the client")
	end
end

-- camelCase keys from the web API as the engine hands them on: PascalCase.
local function pascal(value)
	if type(value) ~= "table" then
		return value
	end
	local out = {}
	for k, v in value do
		if type(k) == "string" then
			k = string.upper(string.sub(k, 1, 1)) .. string.sub(k, 2)
		end
		out[k] = pascal(v)
	end
	return out
end

function EDITOR.GetAvatarRulesAsync(_)
	return pascal(host.web("https://avatar.roblox.com/v1/avatar-rules", "GET", nil, "avatar rules"))
end

local ASSET_TYPES = {}
for _, item in Enum.AssetType:GetEnumItems() do
	ASSET_TYPES[item.Value] = item.Name
end

local function itemType(value)
	if typeof(value) ~= "EnumItem" or value.EnumType ~= Enum.AvatarItemType then
		fail("Unable to cast " .. typeof(value) .. " to AvatarItemType")
	end
	return value.Name
end

local BUNDLE_TYPES = {}
for _, item in Enum.BundleType:GetEnumItems() do
	BUNDLE_TYPES[item.Value] = item.Name
end

-- What the catalog answers that a live server leaves out of an item's
-- details (the AvatarApi golden's `aes_*_shape` fields).
local UNSHOWN = { "ItemCreatedUtc", "RenderProperties", "Taxonomy" }

local function details(raw)
	local out = pascal(raw)
	if type(out.AssetType) == "number" then
		out.AssetType = ASSET_TYPES[out.AssetType] or tostring(out.AssetType)
	end
	if type(out.BundleType) == "number" then
		out.BundleType = BUNDLE_TYPES[out.BundleType] or tostring(out.BundleType)
	end
	for _, key in UNSHOWN do
		out[key] = nil
	end
	return out
end

function EDITOR.GetItemDetailsAsync(_, id, kind)
	id = integer(id)
	kind = itemType(kind)
	return details(
		host.web(
			"https://catalog.roblox.com/v1/catalog/items/" .. id .. "/details?itemType=" .. kind,
			"GET",
			nil,
			"item details"
		)
	)
end

function EDITOR.GetBatchItemDetailsAsync(self, ids, kind)
	if type(ids) ~= "table" then
		fail("Unable to cast " .. typeof(ids) .. " to Array")
	end
	local out = {}
	for _, id in ids do
		table.insert(out, EDITOR.GetItemDetailsAsync(self, id, kind))
	end
	return out
end

function EDITOR.GetOutfitDetailsAsync(_, id)
	id = integer(id)
	return pascal(host.web("https://avatar.roblox.com/v1/outfits/" .. id .. "/details", "GET", nil, "outfit"))
end

function EDITOR.GetRecommendedAssetsAsync(_, assetType, contextAssetId)
	local id = typeof(assetType) == "EnumItem" and assetType.Value
	if not id then
		fail("Unable to cast " .. typeof(assetType) .. " to AvatarAssetType")
	end
	local reply = host.web(
		string.format(
			"https://catalog.roblox.com/v2/recommendations/assets?assetTypeId=%d&contextAssetId=%d&numItems=40",
			id,
			tonumber(contextAssetId) or 0
		),
		"GET",
		nil,
		"recommendations"
	)
	return pascal(reply.data or {})
end

function EDITOR.GetRecommendedBundlesAsync(_, bundleId)
	bundleId = integer(bundleId)
	local reply = host.web(
		"https://catalog.roblox.com/v1/bundles/" .. bundleId .. "/recommendations?numItems=24",
		"GET",
		nil,
		"recommendations"
	)
	return pascal(reply.data or {})
end

-- CatalogPages' state by instance id: the current page, whether it's the
-- last, and for a search its URL and cursor.
local CATALOG_PAGES = {}
local catalog = {}

-- What a bundle's catalog details show in a listing (a search page, the
-- bundles an asset is in): the details less what only a single item's
-- answer has.
local function listed(raw)
	local out = details(raw)
	out.ExpectedSellerId, out.IsPurchasable, out.IsOffSale, out.Owned, out.OffSaleDeadline = nil, nil, nil, nil, nil
	return out
end

-- The bundles holding an asset, one CatalogPages page of them as a
-- catalog search lists items.
function EDITOR.GetBundlesByAssetIdAsync(_, assetId)
	needed(assetId)
	assetId = integer(assetId)
	local reply = host.web(
		"https://catalog.roblox.com/v1/assets/" .. assetId .. "/bundles?limit=100",
		"GET",
		nil,
		"bundles"
	)
	local page = {}
	for _, bundle in reply.data or {} do
		local raw = host.web(
			"https://catalog.roblox.com/v1/catalog/items/" .. integer(bundle.id) .. "/details?itemType=Bundle",
			"GET",
			nil,
			"item details"
		)
		table.insert(page, listed(raw))
	end
	local pages, id = host.new("CatalogPages", "CatalogPages")
	catalog[id] = { page = page, finished = true }
	return pages
end

function EDITOR.GetHeadShapesAsync(_)
	if host.server() then
		fail("HTTP 401")
	end
	return {}
end

-- Default clothing on a description wearing none, as the avatar rules'
-- default lists give it.
function EDITOR.CheckApplyDefaultClothingAsync(_, value)
	local copy = description(value):Clone()
	if copy.Shirt == 0 and copy.Pants == 0 then
		copy.Shirt = 855776103
		copy.Pants = 855783877
	end
	return copy
end

function EDITOR.ConformToAvatarRulesAsync(_, value)
	return description(value):Clone()
end

function EDITOR.GetInventoryAsync(_, assetTypes)
	clientOnly()
	if type(assetTypes) ~= "table" then
		fail("Unable to cast " .. typeof(assetTypes) .. " to Array")
	end
	return host.new("InventoryPages", "InventoryPages")
end

function EDITOR.GetOutfitsAsync(_)
	clientOnly()
	return host.new("OutfitPages", "OutfitPages")
end

-- The catalog's search, a page at a time: CatalogPages over the web
-- catalog's `search/items/details`, its cursor kept per pages object.
local LIMITS = { 10, 28, 30, 50, 60, 100, 120 }

local function catalogPage(state)
	local url = state.url .. (state.cursor and ("&cursor=" .. state.cursor) or "")
	local ok, reply = pcall(host.web, url, "GET", nil, "catalog search")
	if not ok then
		fail("HTTP 400 (Bad Request)")
	end
	state.page = {}
	for _, item in (reply and reply.data) or {} do
		table.insert(state.page, details(item))
	end
	state.cursor = reply and reply.nextPageCursor
	state.finished = state.cursor == nil
end

local function query(params)
	local parts = {}
	local function add(key, value)
		table.insert(parts, key .. "=" .. game:GetService("HttpService"):UrlEncode(tostring(value)))
	end
	if params.SearchKeyword ~= "" then
		add("Keyword", params.SearchKeyword)
	end
	add("Category", params.CategoryFilter.Value)
	add("SortType", params.SortType.Value)
	add("SortAggregation", params.SortAggregation.Value)
	add("SalesTypeFilter", params.SalesTypeFilter.Value)
	if params.MinPrice > 0 then
		add("MinPrice", params.MinPrice)
	end
	if params.MaxPrice < 2147483647 then
		add("MaxPrice", params.MaxPrice)
	end
	if params.IncludeOffSale then
		add("IncludeNotForSale", "true")
	end
	if params.CreatorName ~= "" then
		add("CreatorName", params.CreatorName)
	end
	if params.CreatorId ~= 0 then
		add("CreatorTargetId", params.CreatorId)
	end
	local limit = LIMITS[#LIMITS]
	for _, allowed in LIMITS do
		if allowed >= params.Limit then
			limit = allowed
			break
		end
	end
	add("Limit", limit)
	return "https://catalog.roblox.com/v1/search/items/details?" .. table.concat(parts, "&")
end

function EDITOR.SearchCatalogAsync(_, params)
	if typeof(params) ~= "CatalogSearchParams" then
		fail("Unable to cast " .. typeof(params) .. " to CatalogSearchParams")
	end
	local state = { url = query(params) }
	catalogPage(state)
	local pages, id = host.new("CatalogPages", "CatalogPages")
	catalog[id] = state
	return pages
end

function CATALOG_PAGES.GetCurrentPage(self)
	local state = catalog[host.id(self)]
	return state and table.clone(state.page) or {}
end

function CATALOG_PAGES.AdvanceToNextPageAsync(self)
	local state = catalog[host.id(self)]
	if not state or state.finished then
		fail("No pages to advance to")
	end
	catalogPage(state)
end

local EMPTY_PAGES = {}

function EMPTY_PAGES.GetCurrentPage(_)
	return {}
end

function EMPTY_PAGES.AdvanceToNextPageAsync(_)
	fail("No pages to advance to")
end

local TABLES = {
	Humanoid = HUMANOID,
	HumanoidDescription = DESCRIPTION,
	AccessoryDescription = ACCESSORY_DESCRIPTION,
	Player = PLAYER,
	Players = PLAYERS,
	BanHistoryPages = BAN_HISTORY,
	InventoryPages = EMPTY_PAGES,
	OutfitPages = EMPTY_PAGES,
	CatalogPages = CATALOG_PAGES,
	Teams = TEAMS,
	AvatarCreationService = CREATION,
	AvatarEditorService = EDITOR,
}

local SIGNALS = {
	Humanoid = HUMANOID_SIGNALS,
	HumanoidDescription = DESCRIPTION_SIGNALS,
	Player = PLAYER_SIGNALS,
	Players = PLAYERS_SIGNALS,
	AvatarCreationService = CREATION_SIGNALS,
}

local FINISHED = { BanHistoryPages = true, InventoryPages = true, OutfitPages = true }

function M.member(class, id, key)
	if FINISHED[class] and key == "IsFinished" then
		return true, true
	end
	if class == "CatalogPages" and key == "IsFinished" then
		local state = catalog[id]
		return state == nil or state.finished, true
	end
	local s = SIGNALS[class]
	if s and s[key] then
		return signals(s, id, key)
	end
	local t = TABLES[class]
	local found = t and t[key]
	return found, found ~= nil
end

return M
