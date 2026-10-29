-- The service stand-in of the touch simulation (see touch_input): the
-- proxy over UserInputService, the console variant, and the editor's
-- keyboard cover as the proxy reads it.
local Gesture = require(script.Parent:WaitForChild("__rbxnative_touch_gesture"))

local P = {}

P.SIGNALS = {
	"InputBegan",
	"InputChanged",
	"InputEnded",
	"TouchStarted",
	"TouchMoved",
	"TouchEnded",
	"TouchTap",
	"TouchLongPress",
	"TouchPan",
	"TouchSwipe",
	"TouchPinch",
	"TouchRotate",
	"LastInputTypeChanged",
}

-- The OnScreenKeyboard* values now, from the last `setKeyboard`.
local keyboard = Gesture.keyboardProps(0, 0, 0, 0)

function P.keyboardValue(k)
	local v = keyboard[k]
	if type(v) == "table" then
		return Vector2.new(v[1], v[2])
	end
	return v
end

-- The cover moved: remembers it, returns the properties that changed.
function P.moveKeyboard(x, y, width, height)
	local new = Gesture.keyboardProps(tonumber(x) or 0, tonumber(y) or 0, tonumber(width) or 0, tonumber(height) or 0)
	local changes = Gesture.keyboardChanges(keyboard, new)
	keyboard = new
	return changes
end

-- A UserInputService whose `overrides` win; `read` and `write` see every
-- other key first (`read` returning nil, `write` false: not theirs).
function P.makeProxy(real, overrides, read, write)
	return setmetatable({}, {
		__index = function(_, k)
			local override = overrides[k]
			if override ~= nil then
				return override
			end
			local own = read and read(k)
			if own ~= nil then
				return own
			end
			local value = real[k]
			if type(value) == "function" then
				return function(_, ...)
					return real[k](real, ...)
				end
			end
			return value
		end,
		__newindex = function(_, k, v)
			if not (write and write(k, v)) then
				real[k] = v
			end
		end,
		__tostring = function()
			return "UserInputService"
		end,
	})
end

function P.preferredTouch(real)
	local ok, pref = pcall(function()
		return real.PreferredInput == Enum.PreferredInput.Touch
	end)
	return ok and pref
end

-- A simulated console's client: the mouse stays a mouse, and only
-- PreferredInput reads Gamepad.
function P.console()
	local real = game:GetService("UserInputService")
	local overrides = {}
	pcall(function()
		overrides.PreferredInput = Enum.PreferredInput.Gamepad
	end)
	return { real = real, signals = {}, proxy = P.makeProxy(real, overrides) }
end

function P.tenFoot()
	local ok, yes = pcall(function()
		return game:GetService("GuiService"):IsTenFootInterface()
	end)
	return ok and yes == true
end


return P
