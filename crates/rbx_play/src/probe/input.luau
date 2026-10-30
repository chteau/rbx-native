-- `__rbxnative_input`: the client's half of the MCP input tools.
--
-- The editor sends real mouse and key events to the client's window itself;
-- what only the client knows comes from here, each command answered with an
-- ack: where a GuiObject is on screen (`locate`), walking the character
-- along a computed path (`navigate`, as Studio's character_navigation does:
-- Humanoid:MoveTo along PathfindingService waypoints at a scaled WalkSpeed)
-- and text typed into the focused TextBox (`type_text`: Studio's textInput
-- inserts text without key events, so none are sent).
--
-- Errors are raised with Studio's own wording, level 0 so no position is
-- prepended.

local Input = {}

local function service(name)
	return game:GetService(name)
end

-- A child by exact name, else ignoring case.
local function child(node, name)
	local found = node:FindFirstChild(name)
	if found then
		return found
	end
	local lower = string.lower(name)
	for _, candidate in node:GetChildren() do
		if string.lower(candidate.Name) == lower then
			return candidate
		end
	end
	return nil
end

-- The instance a dotted path names, as Studio's tools read it: from `game`
-- (written or not) or from `LocalPlayer`. A name may itself hold dots, so
-- each level tries every run of segments as one name, shortest first.
function Input.resolveFrom(root, localPlayer, path)
	local segments = string.split(path, ".")
	local node = root
	local at = 1
	if segments[1] == "game" then
		at = 2
	elseif segments[1] == "LocalPlayer" then
		node = localPlayer
		at = 2
	end
	while node and at <= #segments do
		local found = nil
		for last = at, #segments do
			found = child(node, table.concat(segments, ".", at, last))
			if found then
				at = last + 1
				break
			end
		end
		node = found
	end
	return node
end

local function resolve(path)
	local found = Input.resolveFrom(game, service("Players").LocalPlayer, path)
	if not found then
		error("Instance not found: " .. path, 0)
	end
	return found
end

-- `typed` put into `text` at a TextBox's caret: `cursor` and
-- `selectionStart` are byte offsets counted from 1, -1 when unset, and a
-- selection between them is replaced. Returns the text and the new caret.
function Input.insert(text, cursor, selectionStart, typed)
	if cursor < 1 then
		cursor = #text + 1
	end
	local from, to = cursor, cursor
	if selectionStart >= 1 and selectionStart ~= cursor then
		from, to = math.min(selectionStart, cursor), math.max(selectionStart, cursor)
	end
	return string.sub(text, 1, from - 1) .. typed .. string.sub(text, to), from + #typed
end

-- The GUI inset and viewport the editor maps GUI coordinates through, and
-- the centre of the GuiObject at `path` in GUI coordinates (where
-- InputObject.Position and AbsolutePosition are measured).
local function locate(path)
	local camera = workspace.CurrentCamera
	local inset = service("GuiService"):GetGuiInset()
	local answer = {
		inset_x = inset.X,
		inset_y = inset.Y,
		width = camera.ViewportSize.X,
		height = camera.ViewportSize.Y,
	}
	if path then
		local target = resolve(path)
		if not target:IsA("GuiObject") then
			error("Instance is not a GuiObject: " .. target.ClassName, 0)
		end
		answer.x = target.AbsolutePosition.X + target.AbsoluteSize.X / 2
		answer.y = target.AbsolutePosition.Y + target.AbsoluteSize.Y / 2
	end
	return service("HttpService"):JSONEncode(answer)
end

local function chatFocused()
	local ok, focused = pcall(function()
		return service("TextChatService").ChatInputBarConfiguration.IsFocused
	end)
	return ok and focused == true
end

-- How long the click the editor sent to a textInput's target gets to focus
-- it: the engine applies it on a later frame.
local FOCUS_WAIT = 1

-- "typed" into a game TextBox, "chat" when the chat bar has the focus
-- (CoreGui, out of a game script's reach: the editor types it as keys) or
-- "none" when nothing takes text, which Studio ignores too.
local function typeText(typed, path)
	local UserInputService = service("UserInputService")
	if path then
		local target = resolve(path)
		local deadline = os.clock() + FOCUS_WAIT
		while UserInputService:GetFocusedTextBox() ~= target and not chatFocused() and os.clock() < deadline do
			task.wait()
		end
	end
	local box = UserInputService:GetFocusedTextBox()
	if box then
		if box.TextEditable then
			local text, caret = Input.insert(box.Text, box.CursorPosition, box.SelectionStart, typed)
			box.Text = text
			box.CursorPosition = caret
			box.SelectionStart = -1
		end
		return "typed"
	end
	return if chatFocused() then "chat" else "none"
end

local function walk(humanoid, waypoints)
	for index, waypoint in waypoints do
		if index > 1 then
			if waypoint.Action == Enum.PathWaypointAction.Jump then
				humanoid.Jump = true
			end
			-- MoveTo gives up after 8 seconds; one more try gets past a
			-- character that was bumped off its line.
			local reached = false
			for _ = 1, 2 do
				humanoid:MoveTo(waypoint.Position)
				reached = humanoid.MoveToFinished:Wait()
				if reached or humanoid.Health <= 0 then
					break
				end
			end
			if not reached then
				error("The character could not reach the destination", 0)
			end
		end
	end
end

local function navigate(cmd)
	local destination
	if cmd.path then
		local target = resolve(cmd.path)
		if target:IsA("BasePart") then
			destination = target.Position
		elseif target:IsA("Model") then
			destination = target:GetPivot().Position
		else
			error("Instance is not a Model or BasePart: " .. target.ClassName, 0)
		end
	else
		destination = Vector3.new(cmd.x or 0, cmd.y or 0, cmd.z or 0)
	end
	-- Studio's waits for a character and then for its Humanoid, however
	-- long that takes (WaitForChild warns of an infinite yield after 5 s).
	local player = service("Players").LocalPlayer
	local character = player.Character or player.CharacterAdded:Wait()
	local humanoid = character:WaitForChild("Humanoid")
	local root = humanoid.RootPart
	if not root then
		error("The character has not spawned", 0)
	end
	local path = service("PathfindingService"):CreatePath()
	-- A destination too far for one request raises instead of failing.
	local computed = pcall(path.ComputeAsync, path, root.Position, destination)
	if not computed or path.Status ~= Enum.PathStatus.Success then
		error("Can not find a route to the destination", 0)
	end
	local walkSpeed = humanoid.WalkSpeed
	humanoid.WalkSpeed = walkSpeed * math.clamp(cmd.speed or 1, 0.1, 10)
	local ok, err = pcall(walk, humanoid, path:GetWaypoints())
	humanoid.WalkSpeed = walkSpeed
	if not ok then
		error(err, 0)
	end
	return "Success"
end

-- Runs one command; raises its failure.
function Input.handle(cmd)
	if cmd.type == "locate" then
		return locate(cmd.path)
	elseif cmd.type == "navigate" then
		return navigate(cmd)
	end
	return typeText(cmd.text, cmd.path)
end

return Input
