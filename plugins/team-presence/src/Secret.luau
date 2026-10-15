--!strict
--! Shared presence secret stored as an attribute under ServerStorage, so Team Create replicates it to collaborators.

local ChangeHistoryService = game:GetService("ChangeHistoryService")
local HttpService = game:GetService("HttpService")
local ServerStorage = game:GetService("ServerStorage")

local FOLDER = "RbxNativePresence"
local ATTRIBUTE = "PresenceSecret"

local Secret = {}

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
	local folder = ServerStorage:FindFirstChild(FOLDER)
	if folder == nil then
		local created = Instance.new("Folder")
		created.Name = FOLDER
		created.Parent = ServerStorage
		folder = created
	end
	folder:SetAttribute(ATTRIBUTE, secret)
	if id then
		ChangeHistoryService:FinishRecording(id, Enum.FinishRecordingOperation.Commit)
	end
	return secret
end

return Secret
