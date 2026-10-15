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

function Secret.create(): string
	local secret = string.lower(
		string.gsub(HttpService:GenerateGUID(false), "-", "") .. string.gsub(HttpService:GenerateGUID(false), "-", "")
	)
	local id = ChangeHistoryService:TryBeginRecording("Create presence key")
	ensureFolder():SetAttribute(ATTRIBUTE, secret)
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
	return secret
end

-- The relay the place names, if any: a known kind and a non-empty URL.
function Secret.readRelay(): { kind: string, url: string }?
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		return nil
	end
	local kind = folder:GetAttribute(RELAY_KIND)
	local url = folder:GetAttribute(RELAY_URL)
	if (kind == "worker" or kind == "ntfy") and type(url) == "string" and url ~= "" then
		return { kind = kind :: string, url = url }
	end
	return nil
end

-- Owner setup: writes the relay into the place for every collaborator.
function Secret.writeRelay(kind: string, url: string)
	local id = ChangeHistoryService:TryBeginRecording("Set presence relay")
	local folder = ensureFolder()
	folder:SetAttribute(RELAY_KIND, kind)
	folder:SetAttribute(RELAY_URL, url)
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
end

function Secret.clearRelay()
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		return
	end
	local id = ChangeHistoryService:TryBeginRecording("Clear presence relay")
	folder:SetAttribute(RELAY_KIND, nil)
	folder:SetAttribute(RELAY_URL, nil)
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
end

return Secret
