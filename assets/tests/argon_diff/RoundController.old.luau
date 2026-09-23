--!strict
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local RoundConfig = require(ReplicatedStorage.Shared.RoundConfig)
local Remotes = ReplicatedStorage.Remotes

local RoundController = {}
RoundController.__index = RoundController

local INTERMISSION = 15
local ROUND_LENGTH = 120

function RoundController.new()
	local self = setmetatable({}, RoundController)
	self.state = "Waiting"
	self.timeLeft = 0
	return self
end

function RoundController:canStart(): boolean
	return #Players:GetPlayers() >= 2
end

function RoundController:start()
	self.state = "Intermission"
	self.timeLeft = INTERMISSION
	Remotes.RoundState:FireAllClients(self.state, self.timeLeft)
	task.wait(INTERMISSION)
	self.state = "Playing"
	self.timeLeft = ROUND_LENGTH
	Remotes.RoundState:FireAllClients(self.state, self.timeLeft)
	while self.timeLeft > 0 do
		task.wait(1)
		self.timeLeft -= 1
	end
	self:finish()
end

function RoundController:finish()
	self.state = "Waiting"
	Remotes.RoundState:FireAllClients(self.state, 0)
end

function RoundController:reset()
	self.state = "Waiting"
	self.timeLeft = 0
end

function RoundController:getState(): string
	return self.state
end

function RoundController:getTimeLeft(): number
	return self.timeLeft
end

function RoundController:destroy()
	self.state = "Destroyed"
	setmetatable(self, nil)
end

return RoundController
