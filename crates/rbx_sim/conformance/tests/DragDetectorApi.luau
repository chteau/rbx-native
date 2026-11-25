-- DragDetector and UIDragDetector on a server: property defaults, the
-- reference frame, the scripted-style and constraint calls, the events;
-- Highlight's defaults beside them.
local DRAG = {
	"ActivatedCursorIcon", "ApplyAtCenterOfMass", "Axis", "CursorIcon", "DragFrame", "DragStyle", "Enabled", "GamepadModeSwitchKeyCode",
	"KeyboardModeSwitchKeyCode", "MaxActivationDistance", "MaxDragAngle", "MaxDragTranslation", "MaxForce", "MaxTorque", "MinDragAngle",
	"MinDragTranslation", "Orientation", "PermissionPolicy", "ReferenceInstance", "ResponseStyle", "Responsiveness", "RunLocally",
	"SecondaryAxis", "TrackballRadialPullFactor", "TrackballRollFactor", "VRSwitchKeyCode", "WorldAxis", "WorldSecondaryAxis",
}
local UIDRAG = {
	"ActivatedCursorIcon", "BoundingBehavior", "BoundingUI", "CursorIcon", "DragAxis", "DragRelativity", "DragRotation", "DragSpace",
	"DragStyle", "DragUDim2", "Enabled", "MaxDragAngle", "MaxDragTranslation", "MinDragAngle", "MinDragTranslation", "ReferenceUIInstance",
	"ResponseStyle", "Selectable", "SelectionModeDragSpeed", "SelectionModeRotateSpeed", "UIDragSpeedAxisMapping",
}
local HIGHLIGHT = { "Adornee", "DepthMode", "Enabled", "FillColor", "FillTransparency", "OutlineColor", "OutlineTransparency" }

return function(h, origin)
	local r = {}
	local drag = Instance.new("DragDetector")
	h.props(r, "drag", drag, DRAG)
	r.drag_frame_loose = h.try(function()
		return drag:GetReferenceFrame()
	end)
	local part = h.part({ Anchored = true, Position = origin, Orientation = Vector3.new(0, 90, 0) })
	drag.Parent = part
	r.drag_frame = h.try(function()
		local cf = drag:GetReferenceFrame()
		return h.vec(cf.Position) .. " " .. h.vec(cf.LookVector)
	end)
	drag.Axis = Vector3.new(0, 2, 0)
	r.axis_set = h.show(drag.Axis)
	r.world_axis = h.show(drag.WorldAxis)
	drag.Orientation = Vector3.new(0, 0, 90)
	r.world_axis_turned = h.show(drag.WorldAxis)
	r.world_secondary = h.show(drag.WorldSecondaryAxis)
	r.style_fn_plain = h.try(function()
		drag:SetDragStyleFunction(function()
			return CFrame.new()
		end)
	end)
	r.constraint = h.try(function()
		return drag:AddConstraintFunction(1, function(cf)
			return cf
		end)
	end)
	r.permission_fn = h.try(function()
		drag:SetPermissionPolicyFunction(function()
			return true
		end)
	end)
	r.restart = h.try(function()
		drag:RestartDrag()
	end)
	drag.MaxDragTranslation = Vector3.new(-1, 5, 5)
	r.max_translation_negative = h.show(drag.MaxDragTranslation)
	drag.MaxDragAngle = 400
	r.max_angle_big = h.show(drag.MaxDragAngle)
	drag.Responsiveness = -3
	r.responsiveness_negative = h.show(drag.Responsiveness)
	drag.MaxForce = -1
	r.max_force_negative = h.show(drag.MaxForce)
	r.events = h.try(function()
		return typeof(drag.DragStart) .. typeof(drag.DragContinue) .. typeof(drag.DragEnd) .. typeof(drag.MouseClick)
	end)
	r.drag_isa = drag:IsA("ClickDetector")

	local ui = Instance.new("UIDragDetector")
	h.props(r, "ui", ui, UIDRAG)
	local frame = Instance.new("Frame")
	frame.Position = UDim2.new(0.25, 10, 0, 20)
	frame.Rotation = 15
	ui.Parent = frame
	r.ui_reference_position = h.try(function()
		return ui:GetReferencePosition()
	end)
	r.ui_reference_rotation = h.try(function()
		return ui:GetReferenceRotation()
	end)
	r.ui_constraint = h.try(function()
		return ui:AddConstraintFunction(1, function(p, rot)
			return p, rot
		end)
	end)
	r.ui_style_fn = h.try(function()
		ui:SetDragStyleFunction(function()
			return UDim2.new()
		end)
	end)
	r.ui_isa_component = ui:IsA("UIComponent")

	local highlight = Instance.new("Highlight")
	h.props(r, "highlight", highlight, HIGHLIGHT)
	r.highlight_thickness = h.try(function()
		highlight.LineThickness = 5
		return highlight.LineThickness
	end)
	part:Destroy()
	return r
end
