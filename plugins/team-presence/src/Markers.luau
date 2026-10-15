--!strict
--// Renders rbx-native collaborators' selections and cameras inside Studio.
--// Only "rbx-native" users are drawn: Studio already shows Team Create peers.

local Paths = require(script.Parent.Paths)

local FOLDER_NAME = "RbxNativePresence"

local Markers = {}
Markers.__index = Markers

export type Markers = typeof(setmetatable({} :: {
	folder: Folder?,
	camera: Camera?,
	items: { [string]: Instance },
}, Markers))

local function parseColor(hex: string): Color3
	local r, g, b = string.match(hex, "^#(%x%x)(%x%x)(%x%x)$")
	if not r then
		return Color3.new(1, 1, 1)
	end
	return Color3.fromRGB(tonumber(r, 16) :: number, tonumber(g, 16) :: number, tonumber(b, 16) :: number)
end

local function tag<T>(instance: T & Instance): T
	instance.Archivable = false
	return instance
end

function Markers.new(): Markers
	return setmetatable({ folder = nil, camera = nil, items = {} }, Markers)
end

function Markers.ensureFolder(self: Markers): Folder?
	local camera = workspace.CurrentCamera
	if not camera then
		return nil
	end
	if self.camera ~= camera or not self.folder or self.folder.Parent ~= camera then
		self:clear()
		local folder = Instance.new("Folder")
		folder.Name = FOLDER_NAME
		folder.Archivable = false
		folder.Parent = camera
		self.folder = folder
		self.camera = camera
	end
	return self.folder
end

function Markers.clear(self: Markers)
	if self.folder then
		self.folder:Destroy()
	end
	self.folder = nil
	self.camera = nil
	table.clear(self.items)
end

local function makeCamera(folder: Folder, user: any): Part
	local part = Instance.new("Part")
	part.Archivable = false
	part.Anchored = true
	part.CanCollide = false
	part.CanQuery = false
	part.CanTouch = false
	part.Locked = true
	part.Size = Vector3.new(1, 0.6, 1.5)
	part.Material = Enum.Material.Neon
	part.Transparency = 0.3
	part.Name = "Camera"

	local gui = Instance.new("BillboardGui")
	gui.Archivable = false
	gui.Size = UDim2.fromOffset(120, 24)
	gui.StudsOffset = Vector3.new(0, 2, 0)
	gui.AlwaysOnTop = true
	gui.Adornee = part
	gui.Parent = part

	local label = Instance.new("TextLabel")
	label.Archivable = false
	label.Name = "Label"
	label.Size = UDim2.fromScale(1, 1)
	label.BackgroundTransparency = 1
	label.Font = Enum.Font.BuilderSansBold
	label.TextSize = 16
	label.TextStrokeTransparency = 0.5
	label.Parent = gui

	part.Parent = folder
	return part
end

function Markers.render(self: Markers, users: { any })
	local folder = self:ensureFolder()
	if not folder then
		return
	end
	local wanted: { [string]: boolean } = {}

	for _, user in users do
		-- Only rbx-native senders: Studio draws its own collaborators. The
		-- signed-in user in rbx-native is drawn too (another machine).
		if user.client == "rbx-native" then
			local color = parseColor(user.color)
			for index, path in user.selection do
				local target = Paths.resolve(game, path)
				if target then
					local key = `{user.userId}/sel/{index}`
					wanted[key] = true
					local box = self.items[key] :: SelectionBox?
					if not box then
						local created = Instance.new("SelectionBox")
						created.Archivable = false
						created.LineThickness = 0.05
						created.SurfaceTransparency = 1
						created.Parent = folder
						self.items[key] = created
						box = created
					end
					local b = box :: SelectionBox
					b.Adornee = target
					b.Color3 = color
				end
			end
			if user.camera then
				local key = `{user.userId}/cam`
				wanted[key] = true
				local part = self.items[key] :: Part?
				if not part then
					part = makeCamera(folder, user)
					self.items[key] = part :: Part
				end
				local p = part :: Part
				p.Color = color
				p.CFrame = CFrame.new(table.unpack(user.camera))
				local label = p:FindFirstChild("Label", true) :: TextLabel?
				if label then
					label.Text = user.name
					label.TextColor3 = color
				end
			end
		end
	end

	for key, instance in self.items do
		if not wanted[key] then
			instance:Destroy()
			self.items[key] = nil
		end
	end
end

function Markers.destroy(self: Markers)
	self:clear()
end

return Markers
