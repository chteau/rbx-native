--!strict
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local RoundConfig = require(ReplicatedStorage.Shared.RoundConfig)
local VoteController = require(script.Parent.VoteController)
local Remotes = ReplicatedStorage.Remotes

local RoundController = {}
RoundController.__index = RoundController

function RoundController.new()
	local self = setmetatable({}, RoundController)
	self.state = "Waiting"
	self.timeLeft = 0
	return self
end

function RoundController:canStart(): boolean
	return #Players:GetPlayers() >= RoundConfig.MinPlayers
end

function RoundController:start()
	self.state = "Intermission"
	self.timeLeft = RoundConfig.Intermission
	Remotes.RoundState:FireAllClients(self.state, self.timeLeft)
	local map = VoteController.run(RoundConfig.VoteTime)
	task.wait(self.timeLeft)
	if not self:canStart() then
		self:finish("NotEnoughPlayers")
		return
	end
	self.state = "Playing"
	self.timeLeft = RoundConfig.RoundLength
	Remotes.RoundState:FireAllClients(self.state, self.timeLeft, map)
	while self.timeLeft > 0 do
		task.wait(1)
		self.timeLeft -= 1
	end
	self:finish()
end

function RoundController:finish(reason: string?)
	self.state = "Waiting"
	Remotes.RoundState:FireAllClients(self.state, 0, reason)
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
