-- Lists the selection in a dock widget, anchors the selected parts, and puts
-- drag handles on the first selected part.

local Selection = game:GetService("Selection")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local CoreGui = game:GetService("CoreGui")

local toolbar = plugin:CreateToolbar("Inspector")
local button = toolbar:CreateButton("Inspector", "Show the selection inspector", "", "Inspector")

local info = DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 300, 400, 200, 150)
local widget = plugin:CreateDockWidgetPluginGui("SelectionInspector", info)
widget.Title = "Selection Inspector"

local anchorButton = Instance.new("TextButton")
anchorButton.Size = UDim2.new(1, -8, 0, 28)
anchorButton.Position = UDim2.new(0, 4, 0, 4)
anchorButton.Text = "Anchor all"
anchorButton.Parent = widget

local list = Instance.new("ScrollingFrame")
list.Size = UDim2.new(1, -8, 1, -40)
list.Position = UDim2.new(0, 4, 0, 36)
list.BackgroundTransparency = 1
list.CanvasSize = UDim2.new(0, 0, 0, 0)
list.AutomaticCanvasSize = Enum.AutomaticSize.Y
list.Parent = widget

local layout = Instance.new("UIListLayout")
layout.SortOrder = Enum.SortOrder.LayoutOrder
layout.Parent = list

local handles = Instance.new("Handles")
handles.Parent = CoreGui

local dragOrigin = nil

local function refresh()
	for _, child in ipairs(list:GetChildren()) do
		if child:IsA("TextLabel") then
			child:Destroy()
		end
	end

	local selected = Selection:Get()
	for index, instance in ipairs(selected) do
		local label = Instance.new("TextLabel")
		label.Size = UDim2.new(1, 0, 0, 20)
		label.BackgroundTransparency = 1
		label.TextXAlignment = Enum.TextXAlignment.Left
		label.Text = instance.Name .. " (" .. instance.ClassName .. ")"
		label.LayoutOrder = index
		label.Parent = list
	end

	local first = nil
	for _, instance in ipairs(selected) do
		if instance:IsA("BasePart") then
			first = instance
			break
		end
	end
	handles.Adornee = first
end

button.Click:Connect(function()
	widget.Enabled = not widget.Enabled
	button:SetActive(widget.Enabled)
end)

widget:BindToClose(function()
	widget.Enabled = false
	button:SetActive(false)
end)

Selection.SelectionChanged:Connect(refresh)

anchorButton.MouseButton1Click:Connect(function()
	local changed = false
	for _, instance in ipairs(Selection:Get()) do
		if instance:IsA("BasePart") then
			instance.Anchored = true
			changed = true
		end
	end
	if changed then
		ChangeHistoryService:SetWaypoint("Anchor all")
	end
end)

handles.MouseButton1Down:Connect(function()
	local part = handles.Adornee
	if part then
		dragOrigin = part.CFrame
	end
end)

handles.MouseDrag:Connect(function(face, distance)
	local part = handles.Adornee
	if part and dragOrigin then
		local normal = Vector3.FromNormalId(face)
		part.CFrame = dragOrigin * CFrame.new(normal * distance)
	end
end)

handles.MouseButton1Up:Connect(function()
	if dragOrigin then
		dragOrigin = nil
		ChangeHistoryService:SetWaypoint("Move along face")
	end
end)

plugin.Unloading:Connect(function()
	handles:Destroy()
end)

refresh()
