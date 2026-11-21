--!nonstrict
-- The animation data API a Local Run answers in Luau: KeyframeSequence,
-- Keyframe and Pose as trees of children, the providers' registrations,
-- the Animator and AnimationTrack members the simulation leaves to us,
-- the rig descriptions, IKControl's readings and AnimationNodeDefinition's
-- pins. Our own, from the creator docs and the AnimationApi golden.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

-- A fresh array in the same order.
local function copy(list)
	local out = {}
	for i, v in list do
		out[i] = v
	end
	return out
end

local function children(self, class)
	local out = {}
	for _, child in self:GetChildren() do
		if child:IsA(class) then
			table.insert(out, child)
		end
	end
	return out
end

-- `Add<What>(instance)`: parents an instance of `class` to self; anything
-- else, nil included, is ignored as a live server does.
local function adder(class)
	return function(self, instance)
		if typeof(instance) == "Instance" and instance:IsA(class) then
			instance.Parent = self
		end
	end
end

-- `Remove<What>(instance)`: unparents it if it's a child of self.
local function remover(class)
	return function(self, instance)
		if typeof(instance) == "Instance" and instance:IsA(class) and instance.Parent == self then
			instance.Parent = nil
		end
	end
end

local function getter(class)
	return function(self)
		return children(self, class)
	end
end

local SEQUENCE = {
	AddKeyframe = adder("Keyframe"),
	GetKeyframes = getter("Keyframe"),
	RemoveKeyframe = remover("Keyframe"),
}

-- A NumberPose goes in, but GetPoses lists only Poses (AnimationApi
-- `kf_poses`).
local KEYFRAME = {
	AddPose = adder("PoseBase"),
	GetPoses = getter("Pose"),
	RemovePose = remover("PoseBase"),
	AddMarker = adder("KeyframeMarker"),
	GetMarkers = getter("KeyframeMarker"),
	RemoveMarker = remover("KeyframeMarker"),
}

local POSE = {
	AddSubPose = adder("PoseBase"),
	GetSubPoses = getter("Pose"),
	RemoveSubPose = remover("PoseBase"),
}

-- A live server refuses the registrations and words a failed load so.
local PROVIDER = {}

function PROVIDER.RegisterActiveKeyframeSequence(_)
	fail("Usage error: RegisterActiveKeyframeSequence can only be used in Solo mode.")
end

function PROVIDER.GetKeyframeSequenceAsync(self, id)
	local ok, found = pcall(self["GetKeyframeSequenceAsync#fetch"], self, tostring(id))
	if not ok or found == nil then
		-- The `%s` is literal in Roblox's message.
		fail("Animation failed to load, assetId: %s")
	end
	return found
end

function PROVIDER.GetAnimationsAsync(_, userId)
	if type(userId) ~= "number" then
		fail("Unable to cast " .. typeof(userId) .. " to int64")
	end
	return (host.new("InventoryPages", "InventoryPages"))
end

local CLIP_PROVIDER = {}

function CLIP_PROVIDER.RegisterAnimationClip(_)
	fail("Usage error: RegisterAnimationClip can only be called in Studio.")
end

function CLIP_PROVIDER.RegisterActiveAnimationClip(_)
	fail("Usage error: RegisterActiveKeyframeSequence can only be used in Solo mode.")
end

function CLIP_PROVIDER.GetAnimationClipAsync(self, id)
	local text = tostring(id)
	local ok, found = pcall(self["GetAnimationClipAsync#fetch"], self, text)
	if not ok or found == nil then
		fail(
			string.format(
				"Animation failed to load, assetId: https://assetdelivery.roblox.com/v1/asset?id=%s&serverplaceid=%d",
				string.match(text, "%d+") or text,
				game.PlaceId
			)
		)
	end
	return found
end

CLIP_PROVIDER.GetAnimationsAsync = PROVIDER.GetAnimationsAsync

-- Past the argument check a live server makes (AvatarApi `acp_evaluator`),
-- clip evaluators aren't modelled.
function CLIP_PROVIDER.GetClipEvaluatorAsync(_, clip)
	if clip == nil then
		fail("Argument 1 missing or nil")
	end
	fail("AnimationClipProvider:GetClipEvaluatorAsync is not available in a Local Run")
end

local ANIMATOR = {}

function ANIMATOR.ApplyJointVelocities(_, _motors) end

function ANIMATOR.StepAnimations(_)
	fail("The current thread cannot call 'StepAnimations' (lacking capability PluginOrOpenCloud)")
end

function ANIMATOR.RegisterEvaluationParallelCallback(_)
	fail("RegisterEvaluationParallelCallback is not enabled!")
end

local TRACK = {}

function TRACK.GetParameterDefaults(_)
	return {}
end

function TRACK.GetParameter(_, _key)
	return nil
end

function TRACK.SetParameter(_, _key, _value) end

for _, name in { "GetTargetNames", "GetTargetInstance", "SetTargetInstance" } do
	TRACK[name] = function()
		fail("AnimationTrack:" .. name .. " is not available yet!")
	end
end

-- Rig descriptions: a joint per `Enum.RigLabel`, kept in the property of
-- that name.
local R15_JOINTS = {
	"Root",
	"Waist",
	"Neck",
	"LeftShoulder",
	"LeftElbow",
	"LeftWrist",
	"RightShoulder",
	"RightElbow",
	"RightWrist",
	"LeftHip",
	"LeftKnee",
	"LeftAnkle",
	"RightHip",
	"RightKnee",
	"RightAnkle",
}
local R6_JOINTS = { "Root", "Neck", "LeftShoulder", "RightShoulder", "LeftHip", "RightHip" }
-- Every joint, from the root out (the golden prints arrays by string
-- index, so its listing reads shuffled).
local ALL_JOINTS = {
	"Root",
	"Waist",
	"Spine",
	"Chest",
	"Neck",
	"HeadBase",
	"LeftClavicle",
	"LeftShoulder",
	"LeftElbow",
	"LeftWrist",
	"RightClavicle",
	"RightShoulder",
	"RightElbow",
	"RightWrist",
	"LeftHip",
	"LeftKnee",
	"LeftAnkle",
	"LeftToeBase",
	"RightHip",
	"RightKnee",
	"RightAnkle",
	"RightToeBase",
}

local function label(value)
	if typeof(value) ~= "EnumItem" or value.EnumType ~= Enum.RigLabel then
		fail("Unable to cast " .. typeof(value) .. " to token")
	end
	return value.Name
end

local function property(self, name)
	local ok, value = pcall(function()
		return self[name]
	end)
	return ok and value or nil
end

local RIG = {}

function RIG.GetR15JointNames(_)
	return copy(R15_JOINTS)
end

function RIG.GetR6JointNames(_)
	return copy(R6_JOINTS)
end

function RIG.GetJointNames(_)
	return copy(ALL_JOINTS)
end

function RIG.GetR15JointLabels(_)
	fail("The current thread cannot call 'GetR15JointLabels' (lacking capability PluginOrOpenCloud)")
end

function RIG.GetJoint(self, rigLabel)
	return property(self, label(rigLabel))
end

function RIG.GetJointFromName(self, name)
	for _, joint in ALL_JOINTS do
		local found = property(self, joint)
		if found and found.Name == name then
			return found
		end
	end
	return nil
end

function RIG.SetJoint(self, rigLabel, joint)
	self[label(rigLabel)] = joint
end

function RIG.SetTposeAdjustment(self, rigLabel, transform)
	self[label(rigLabel) .. "TposeAdjustment"] = transform
end

local function zero()
	return Vector3.zero
end

local function identity()
	return CFrame.identity
end

local function none() end

-- A live server reads every finger back as zero and keeps no write.
local DIGITS = {
	GetFingerTip = zero,
	GetFingerControl = zero,
	GetFingerTipRange = zero,
	SetFingerTip = none,
	SetFingerControl = none,
	SetJoint = function(self, rigLabel, joint)
		pcall(function()
			self[label(rigLabel)] = joint
		end)
	end,
}

-- With no solve run on a server, the chain reads as empty.
local IK = {
	GetChainCount = function()
		return 0
	end,
	GetChainLength = function()
		return 0
	end,
	GetNodeLocalCFrame = identity,
	GetNodeWorldCFrame = identity,
	GetRawFinalTarget = identity,
	GetSmoothedFinalTarget = identity,
}

-- AnimationNodeDefinition's input pins, by node; duplicates are allowed
-- and every call fires InputPinsChanged.
local pins = {}
local NODE = {}

local function changed(self)
	host.fire(host.id(self), "InputPinsChanged")
end

local function pinsOf(self)
	local id = host.id(self)
	pins[id] = pins[id] or {}
	return pins[id]
end

function NODE.GetOrderedInputPinNames(self)
	return copy(pinsOf(self))
end

function NODE.AddInputPin(self, pin)
	table.insert(pinsOf(self), tostring(pin))
	changed(self)
end

function NODE.RemoveInputPin(self, pin)
	local list = pinsOf(self)
	local i = table.find(list, tostring(pin))
	if i then
		table.remove(list, i)
	end
	changed(self)
end

function NODE.SetOrderedInputPinNames(self, names)
	if type(names) ~= "table" then
		fail("Unable to cast " .. typeof(names) .. " to Array")
	end
	local list = {}
	for _, name in names do
		table.insert(list, tostring(name))
	end
	pins[host.id(self)] = list
	changed(self)
end

local TABLES = {
	KeyframeSequence = SEQUENCE,
	Keyframe = KEYFRAME,
	Pose = POSE,
	KeyframeSequenceProvider = PROVIDER,
	AnimationClipProvider = CLIP_PROVIDER,
	Animator = ANIMATOR,
	AnimationTrack = TRACK,
	HumanoidRigDescription = RIG,
	DigitsRigDescription = DIGITS,
	IKControl = IK,
	AnimationNodeDefinition = NODE,
}

function M.member(class, id, key)
	if class == "AnimationNodeDefinition" and key == "InputPinsChanged" then
		return host.signal(id, key), true
	end
	local t = TABLES[class]
	local found = t and t[key]
	return found, found ~= nil
end

return M
