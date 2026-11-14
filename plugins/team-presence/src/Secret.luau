--!strict
--! Shared presence secret stored as an attribute under ServerStorage, so Team Create replicates it to collaborators.

local ChangeHistoryService = game:GetService("ChangeHistoryService")
local HttpService = game:GetService("HttpService")
local ServerStorage = game:GetService("ServerStorage")

local FOLDER = "RbxNativePresence"
local ATTRIBUTE = "PresenceSecret"
local RELAY_KIND = "RelayKind"
local RELAY_URL = "RelayUrl"

local Secret = {}

local function ensureFolder(): Folder
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		local created = Instance.new("Folder")
		created.Name = FOLDER
		created.Parent = ServerStorage
		folder = created
	end
	return folder :: Folder
end

function Secret.read(): string?
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		return nil
	end
	local value = folder:GetAttribute(ATTRIBUTE)
	if type(value) == "string" and #value == 64 and string.match(value, "^[0-9a-f]+$") then
		return value
	end
	return nil
end

-- Turning presence on puts everything collaborators need into the place,
-- in one undo step: the key (made here when the place has none) and the
-- relay (`kind`, `url`, when the place names none). Returns the key and
-- whether it was just made. Both reach collaborators with the next save.
function Secret.ensure(kind: string, url: string): (string, boolean)
	local secret = Secret.read()
	local relay = Secret.readRelay()
	if secret ~= nil and relay ~= nil then
		return secret, false
	end
	local id = ChangeHistoryService:TryBeginRecording("Set up presence")
	local folder = ensureFolder()
	local minted = secret == nil
	if secret == nil then
		secret = string.lower(
			string.gsub(HttpService:GenerateGUID(false), "-", "")
				.. string.gsub(HttpService:GenerateGUID(false), "-", "")
		)
		folder:SetAttribute(ATTRIBUTE, secret)
	end
	if relay == nil then
		folder:SetAttribute(RELAY_KIND, kind)
		folder:SetAttribute(RELAY_URL, url)
	end
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
	return secret :: string, minted
end

-- The relay the place names, if any: a known kind and a non-empty URL.
function Secret.readRelay(): { kind: string, url: string }?
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		return nil
	end
	local kind = folder:GetAttribute(RELAY_KIND)
	local url = folder:GetAttribute(RELAY_URL)
	if (kind == "hosted" or kind == "worker" or kind == "ntfy") and type(url) == "string" and url ~= "" then
		return { kind = kind :: string, url = url }
	end
	return nil
end

-- Writes the relay into the place for every collaborator.
function Secret.writeRelay(kind: string, url: string)
	local id = ChangeHistoryService:TryBeginRecording("Set presence relay")
	local folder = ensureFolder()
	folder:SetAttribute(RELAY_KIND, kind)
	folder:SetAttribute(RELAY_URL, url)
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
end

return Secret
