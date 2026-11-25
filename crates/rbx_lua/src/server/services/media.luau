--!nonstrict
-- SoundService's listener and input device, VRService, HapticService and
-- VideoService on a Local Run peer, past what the simulation answers
-- (SoundService:PlayLocalSound and GetMixerTime). Local Run is never in
-- VR and has no microphone or video sampler: VRService answers as a
-- desktop client does (identity CFrames, nothing enabled), the motors
-- only exist on an emulated console's gamepads.
local host = ...

local M = {}

local function fail(message)
	error(message, 0)
end

local function typeName(value)
	if value == nil then
		return "nil"
	end
	if type(value) == "number" then
		return "double"
	end
	return typeof(value)
end

local function enumArg(value, enum, n)
	if typeof(value) ~= "EnumItem" or value.EnumType ~= enum then
		if value == nil then
			fail(`Argument {n} missing or nil`)
		end
		fail(`Unable to cast {typeName(value)} to token`)
	end
	return value
end

local SOUND = {}

function SOUND.GetListener(service)
	local kind = service.ListenerType
	if kind == Enum.ListenerType.CFrame then
		return kind, service.ListenerCFrame
	elseif kind == Enum.ListenerType.Camera then
		return kind, nil
	end
	return kind, service.ListenerObject
end

function SOUND.SetListener(service, kind, listener)
	enumArg(kind, Enum.ListenerType, 1)
	if kind == Enum.ListenerType.CFrame then
		if typeof(listener) ~= "CFrame" then
			fail("SetListener: CFrame expected for ListenerType.CFrame")
		end
		service.ListenerCFrame = listener
	elseif kind ~= Enum.ListenerType.Camera then
		if typeof(listener) ~= "Instance" then
			fail("SetListener: Instance expected for ListenerType." .. kind.Name)
		end
		service.ListenerObject = listener
	end
	service.ListenerType = kind
end

-- No microphone here: no device, muted.
function SOUND.GetInputDevice(_)
	return "", "", true, {}, {}, 0
end

function SOUND.SetInputDevice(_, device)
	if device == nil then
		fail("Argument 1 missing or nil")
	end
end

local touchpads = {}
local DEFAULT_TOUCHPAD = {
	Left = Enum.VRTouchpadMode.VirtualThumbstick,
	Right = Enum.VRTouchpadMode.ABXY,
}

local VR = {}

function VR.GetUserCFrame(_, kind)
	enumArg(kind, Enum.UserCFrame, 1)
	return CFrame.identity
end

function VR.GetUserCFrameEnabled(_, kind)
	enumArg(kind, Enum.UserCFrame, 1)
	return false
end

function VR.GetTouchpadMode(_, pad)
	enumArg(pad, Enum.VRTouchpad, 1)
	return touchpads[pad] or DEFAULT_TOUCHPAD[pad.Name]
end

function VR.SetTouchpadMode(service, pad, mode)
	enumArg(pad, Enum.VRTouchpad, 1)
	enumArg(mode, Enum.VRTouchpadMode, 2)
	if VR.GetTouchpadMode(service, pad) ~= mode then
		touchpads[pad] = mode
		host.fire(host.id(service), "TouchpadModeChanged", pad, mode)
	end
end

function VR.RequestNavigation(service, cframe, kind)
	if typeof(cframe) ~= "CFrame" then
		fail(`Unable to cast {typeName(cframe)} to CoordinateFrame`)
	end
	enumArg(kind, Enum.UserCFrame, 2)
	host.fire(host.id(service), "NavigationRequested", cframe, kind)
end

function VR.RecenterUserHeadCFrame(_) end

local VR_SIGNALS = {
	NavigationRequested = true,
	TouchpadModeChanged = true,
	UserCFrameChanged = true,
	UserCFrameEnabled = true,
}

-- What SetMotor left, by input type then motor.
local motors = {}

local function vibrates(kind)
	return host.device().gamepad and string.match(kind.Name, "^Gamepad%d$") ~= nil
end

local HAPTIC = {}

function HAPTIC.IsVibrationSupported(_, kind)
	return vibrates(enumArg(kind, Enum.UserInputType, 1))
end

function HAPTIC.IsMotorSupported(_, kind, motor)
	enumArg(kind, Enum.UserInputType, 1)
	enumArg(motor, Enum.VibrationMotor, 2)
	return vibrates(kind) and (motor == Enum.VibrationMotor.Large or motor == Enum.VibrationMotor.Small)
end

function HAPTIC.SetMotor(service, kind, motor, ...)
	if HAPTIC.IsMotorSupported(service, kind, motor) then
		motors[kind] = motors[kind] or {}
		motors[kind][motor] = table.pack(...)
	end
end

-- Nothing until SetMotor gave the motor a value.
function HAPTIC.GetMotor(_, kind, motor)
	enumArg(kind, Enum.UserInputType, 1)
	enumArg(motor, Enum.VibrationMotor, 2)
	local values = motors[kind] and motors[kind][motor]
	if values then
		return table.unpack(values, 1, values.n)
	end
end

local VIDEO = {}

function VIDEO.CreateVideoSamplerAsync(_, content)
	if typeof(content) ~= "Content" then
		fail(`Unable to cast {typeName(content)} to Content`)
	end
	fail("CreateVideoSamplerAsync failed: video decoding is not available in Local Run")
end

local MEMBERS = {
	SoundService = SOUND,
	VRService = VR,
	HapticService = HAPTIC,
	VideoService = VIDEO,
}

function M.member(class, id, key)
	if class == "VRService" and VR_SIGNALS[key] then
		return host.signal(id, key), true
	end
	local member = MEMBERS[class][key]
	return member, member ~= nil
end

return M
