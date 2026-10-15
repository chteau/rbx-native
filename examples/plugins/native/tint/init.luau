local Selection = game:GetService("Selection")

rbxnative.command("tint", "Tint", function()
	local color = plugin:PromptColor3()
	if not color then
		return
	end

	local count = 0
	for _, instance in ipairs(Selection:Get()) do
		if instance:IsA("BasePart") then
			instance.Color = color
			count += 1
		end
	end
	rbxnative.notify("Tinted " .. count .. " part(s)")
end)
