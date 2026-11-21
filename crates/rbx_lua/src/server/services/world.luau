--!nonstrict
-- The 3D world's members past the engine's own: Workspace's clocks and
-- counters and its joins, WorldRoot:BulkMoveTo and ArePartsTouchingOthers,
-- BasePart surface helpers, Model scaling, moving and persistent players,
-- the Camera's world queries, Beam:SetTextureOffset and Path2D's curves.
-- Our own, from the creator docs and the WorldApi and Path2DCurve goldens.
local host = ...

local M = {}

local function isA(instance, class)
	return typeof(instance) == "Instance" and instance:IsA(class)
end

local function check(value, kind, n, name)
	if typeof(value) ~= kind then
		error(string.format("Argument %d missing or nil", n), 3)
	end
	return value
end

-- Workspace -------------------------------------------------------------

local WORKSPACE = {}

function WORKSPACE.GetServerTimeNow()
	return host.epoch()
end

-- A server steps physics at 60 Hz when it keeps up.
function WORKSPACE.GetRealPhysicsFPS()
	return 60
end

-- The percentage of real time physics runs at: never throttled here.
function WORKSPACE.GetPhysicsThrottling()
	return 100
end

function WORKSPACE.PGSIsEnabled()
	return true
end

local function partsOf(objects)
	local parts, set = {}, {}
	local function add(p)
		if not set[p] then
			set[p] = true
			table.insert(parts, p)
		end
	end
	for _, o in objects do
		if isA(o, "BasePart") then
			add(o)
		end
		if typeof(o) == "Instance" then
			for _, d in o:GetDescendants() do
				if d:IsA("BasePart") then
					add(d)
				end
			end
		end
	end
	return parts, set
end

-- Parts `p` touches face to face or overlaps, other than those in `set`.
local GAP = 0.01
local function touchingOutside(p, set)
	local params = OverlapParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = { p }
	local found = {}
	for _, other in workspace:GetPartBoundsInBox(p.CFrame, p.Size + Vector3.one * GAP * 2, params) do
		if not set[other] and other ~= workspace.Terrain then
			table.insert(found, other)
		end
	end
	return found
end

-- Welds `objects` to what they touch outside them, a Weld in the part
-- for each pair; `Surface` joins the same, `None` nothing.
function WORKSPACE.JoinToOutsiders(self, objects, mode)
	check(objects, "table", 1)
	local name = if typeof(mode) == "EnumItem" then mode.Name else "All"
	if name == "None" then
		return
	end
	local parts, set = partsOf(objects)
	for _, p in parts do
		for _, other in touchingOutside(p, set) do
			local weld = Instance.new("Weld")
			weld.Part0 = p
			weld.Part1 = other
			weld.C0 = p.CFrame:ToObjectSpace(other.CFrame)
			weld.Parent = p
		end
	end
end

local function jointParts(j)
	if j:IsA("JointInstance") or j:IsA("WeldConstraint") or j:IsA("NoCollisionConstraint") then
		return j.Part0, j.Part1
	end
	return nil, nil
end

function WORKSPACE.UnjoinFromOutsiders(self, objects)
	check(objects, "table", 1)
	local parts, set = partsOf(objects)
	for _, p in parts do
		for _, j in p:GetJoints() do
			local a, b = jointParts(j)
			if a and b and (set[a] ~= set[b]) then
				j:Destroy()
			end
		end
	end
end

-- WorldRoot -------------------------------------------------------------

local WORLD_ROOT = {}

function WORLD_ROOT.BulkMoveTo(self, partList, cframeList, mode)
	check(partList, "table", 1)
	check(cframeList, "table", 2)
	if #partList ~= #cframeList then
		error(string.format("Length of part list (%d) and length of CFrame list (%d) do not match.", #partList, #cframeList), 2)
	end
	for i, p in partList do
		if not isA(p, "BasePart") then
			local kind = if typeof(p) == "Instance" then p.ClassName else typeof(p)
			error(string.format("Can only call BulkMoveTo on BaseParts, partList[%d] is a %s.", i, kind), 2)
		end
		if typeof(cframeList[i]) ~= "CFrame" then
			error(string.format("Unable to cast value to CFrame at cframeList[%d].", i), 2)
		end
	end
	for i, p in partList do
		p.CFrame = cframeList[i]
	end
end

-- Whether any part in `partList` overlaps a part outside it by more than
-- `overlapIgnored` studs: parts that only share a face don't count.
function WORLD_ROOT.ArePartsTouchingOthers(self, partList, overlapIgnored)
	check(partList, "table", 1)
	local ignored = overlapIgnored or 0.000199999995
	local set = {}
	for _, p in partList do
		set[p] = true
	end
	local params = OverlapParams.new()
	params.FilterType = Enum.RaycastFilterType.Exclude
	params.FilterDescendantsInstances = partList
	for _, p in partList do
		if isA(p, "BasePart") then
			local shrunk = p.Size - Vector3.one * ignored * 2
			if shrunk.X > 0 and shrunk.Y > 0 and shrunk.Z > 0 then
				local near = {}
				for _, other in workspace:GetPartBoundsInBox(p.CFrame, shrunk, params) do
					near[other] = true
				end
				for _, other in workspace:GetPartsInPart(p, params) do
					if near[other] and not set[other] then
						return true
					end
				end
			end
		end
	end
	return false
end

-- BasePart --------------------------------------------------------------

local BASE_PART = {}

local function shapeOf(p)
	if p:IsA("Part") then
		return p.Shape.Name
	end
	return if p:IsA("WedgePart") then "Wedge" else "Block"
end

-- The closest point of `p`'s surface to `position`, or `position` itself
-- when it is inside.
function BASE_PART.GetClosestPointOnSurface(p, position)
	check(position, "Vector3", 1)
	local cf, size = p.CFrame, p.Size
	local l = cf:PointToObjectSpace(position)
	local h = size / 2
	local shape = shapeOf(p)
	if shape == "Ball" then
		local r = math.min(size.X, size.Y, size.Z) / 2
		if l.Magnitude <= r then
			return position
		end
		return cf:PointToWorldSpace(l.Unit * r)
	elseif shape == "Cylinder" then
		local r = math.min(size.Y, size.Z) / 2
		local radial = Vector3.new(0, l.Y, l.Z)
		local x = math.clamp(l.X, -h.X, h.X)
		if radial.Magnitude <= r and math.abs(l.X) <= h.X then
			return position
		end
		if radial.Magnitude > r then
			radial = radial.Unit * r
		end
		return cf:PointToWorldSpace(Vector3.new(x, radial.Y, radial.Z))
	end
	local clamped = Vector3.new(math.clamp(l.X, -h.X, h.X), math.clamp(l.Y, -h.Y, h.Y), math.clamp(l.Z, -h.Z, h.Z))
	return cf:PointToWorldSpace(clamped)
end

local FACE_AXIS = {
	Right = Vector3.xAxis,
	Left = -Vector3.xAxis,
	Top = Vector3.yAxis,
	Bottom = -Vector3.yAxis,
	Back = Vector3.zAxis,
	Front = -Vector3.zAxis,
}

-- Grows (or shrinks) `p` by `delta` studs on one face, moving it by half
-- that so the opposite face stays put, as Studio's resize handles do.
-- Other parts don't stop it, and a size that would go under the minimum
-- stops at it while the part still moves the whole way.
local MIN_SIZE = 0.001
function BASE_PART.Resize(p, normalId, delta)
	if typeof(normalId) ~= "EnumItem" or normalId.EnumType ~= Enum.NormalId then
		error("Unable to cast value to NormalId", 2)
	end
	delta = math.floor(check(delta, "number", 2))
	local axis = FACE_AXIS[normalId.Name]
	local along = Vector3.new(math.abs(axis.X), math.abs(axis.Y), math.abs(axis.Z))
	local size = (p.Size + along * delta):Max(Vector3.one * MIN_SIZE)
	local cf = p.CFrame * CFrame.new(axis * delta / 2)
	p.Size = size
	p.CFrame = cf
	return true
end

local BASE_PART_PROPS = {}

function BASE_PART_PROPS.ResizeableFaces()
	return Faces.new(
		Enum.NormalId.Right,
		Enum.NormalId.Top,
		Enum.NormalId.Back,
		Enum.NormalId.Left,
		Enum.NormalId.Bottom,
		Enum.NormalId.Front
	)
end

function BASE_PART_PROPS.ResizeIncrement(p)
	return if p:IsA("TrussPart") then 2 else 1
end

-- The box physics holds the part in: its own size and frame.
function BASE_PART_PROPS.ExtentsSize(p)
	return p.Size
end

function BASE_PART_PROPS.ExtentsCFrame(p)
	return p.CFrame
end

function BASE_PART_PROPS.CurrentPhysicalProperties(p)
	return p.CustomPhysicalProperties or PhysicalProperties.new(p.Material)
end

-- Model -----------------------------------------------------------------

local MODEL = {}
local persistent = {}

function MODEL.GetScale(model)
	return host.raw_get(host.id(model), "ScaleFactor") or 1
end

local function scaleSequence(seq, f)
	local points = {}
	for _, k in seq.Keypoints do
		table.insert(points, NumberSequenceKeypoint.new(k.Time, k.Value * f, k.Envelope * f))
	end
	return NumberSequence.new(points)
end

local LENGTHS = {
	RopeConstraint = { "Length" },
	RodConstraint = { "Length" },
	SpringConstraint = { "FreeLength", "MinLength", "MaxLength" },
	PrismaticConstraint = { "LowerLimit", "UpperLimit" },
	CylindricalConstraint = { "LowerLimit", "UpperLimit" },
	Fire = { "Size" },
	Smoke = { "Size" },
	Explosion = { "BlastRadius" },
	Humanoid = { "HipHeight" },
	Beam = { "Width0", "Width1", "CurveSize0", "CurveSize1" },
}

-- Everything under `model` scaled by `f` about `pivot`'s position.
local function scaleUnder(model, f, pivot)
	local at = pivot.Position
	local function moved(cf)
		return cf - cf.Position + at + (cf.Position - at) * f
	end
	for _, d in model:GetDescendants() do
		if d:IsA("BasePart") then
			d.Size *= f
			d.CFrame = moved(d.CFrame)
			d.PivotOffset = d.PivotOffset - d.PivotOffset.Position + d.PivotOffset.Position * f
		elseif d:IsA("Attachment") then
			d.CFrame = d.CFrame - d.CFrame.Position + d.CFrame.Position * f
		elseif d:IsA("JointInstance") then
			d.C0 = d.C0 - d.C0.Position + d.C0.Position * f
			d.C1 = d.C1 - d.C1.Position + d.C1.Position * f
		elseif d:IsA("DataModelMesh") then
			-- The mesh follows its part's size; only where it sits scales.
			d.Offset *= f
		elseif d:IsA("Model") then
			d.WorldPivot = moved(d.WorldPivot)
		elseif d:IsA("ParticleEmitter") then
			d.Size = scaleSequence(d.Size, f)
			d.Speed = NumberRange.new(d.Speed.Min * f, d.Speed.Max * f)
			d.Acceleration *= f
		elseif d:IsA("Trail") then
			d.WidthScale = scaleSequence(d.WidthScale, f)
		end
		for _, prop in LENGTHS[d.ClassName] or {} do
			d[prop] *= f
		end
	end
end

function MODEL.ScaleTo(model, scale)
	check(scale, "number", 1)
	if scale <= 0 then
		error("Only positive non-zero scale values are allowed.", 2)
	end
	local f = scale / MODEL.GetScale(model)
	if f ~= 1 then
		scaleUnder(model, f, model:GetPivot())
	end
	host.raw_set(host.id(model), "ScaleFactor", scale)
end

-- A model with nothing to move stays as it is.
function MODEL.TranslateBy(model, delta)
	check(delta, "Vector3", 1)
	local ok, pivot = pcall(model.GetPivot, model)
	if ok then
		model:PivotTo(pivot + delta)
	end
end

local function players(model)
	local id = host.id(model)
	persistent[id] = persistent[id] or {}
	return persistent[id]
end

local NO_PLAYER = "PersistentPerPlayer player not specified."

function MODEL.AddPersistentPlayer(model, player)
	if player == nil then
		error(NO_PLAYER, 2)
	end
	if not isA(player, "Player") then
		error("Unable to cast value to Object", 2)
	end
	local list = players(model)
	if not table.find(list, player) then
		table.insert(list, player)
	end
end

function MODEL.RemovePersistentPlayer(model, player)
	if player == nil then
		error(NO_PLAYER, 2)
	end
	local list = players(model)
	local i = player and table.find(list, player)
	if i then
		table.remove(list, i)
	end
end

function MODEL.GetPersistentPlayers(model)
	local out = {}
	for _, p in players(model) do
		if p.Parent then
			table.insert(out, p)
		end
	end
	return out
end

-- Camera ----------------------------------------------------------------

local CAMERA = {}

-- Every part between the camera and any of `castPoints`, in the order the
-- rays meet them, `ignoreList` and its descendants left out.
function CAMERA.GetPartsObscuringTarget(camera, castPoints, ignoreList)
	check(castPoints, "table", 1)
	local from = camera.CFrame.Position
	local found, seen = {}, {}
	for _, point in castPoints do
		local skip = table.clone(ignoreList or {})
		local params = RaycastParams.new()
		params.FilterType = Enum.RaycastFilterType.Exclude
		while true do
			params.FilterDescendantsInstances = skip
			local hit = workspace:Raycast(from, point - from, params)
			if not hit then
				break
			end
			if not seen[hit.Instance] then
				seen[hit.Instance] = true
				table.insert(found, hit.Instance)
			end
			table.insert(skip, hit.Instance)
		end
	end
	return found
end

-- Keeps the camera's direction and backs it off along it until the box's
-- bounding sphere fits the vertical field of view; the focus goes to the
-- box's centre.
function CAMERA.ZoomToExtents(camera, boxCFrame, boxSize)
	check(boxCFrame, "CFrame", 1)
	check(boxSize, "Vector3", 2)
	local radius = boxSize.Magnitude / 2
	local half = math.rad(camera.FieldOfView) / 2
	local distance = radius / math.sin(half)
	local look = camera.CFrame.LookVector
	local center = boxCFrame.Position
	local rotation = camera.CFrame - camera.CFrame.Position
	camera.CFrame = rotation + center - look * distance
	camera.Focus = CFrame.new(center)
end

-- Terrain ---------------------------------------------------------------

-- Material slots: what material, variant and colour each slot stands for,
-- all unset until a script sets them.
local TERRAIN = {}
local SLOTS = 256
local slots = {}

local function slotIndex(index)
	if typeof(index) ~= "number" or index < 0 or index >= SLOTS or index % 1 ~= 0 then
		error("slotIndex is out of range", 3)
	end
	return index
end

function TERRAIN.GetMaterialSlot(_, index)
	local slot = slots[slotIndex(index)]
	if slot then
		return slot[1], slot[2], slot[3]
	end
	return Enum.Material.Air, "", Color3.new()
end

function TERRAIN.SetMaterialSlot(_, index, material, variant, color)
	index = slotIndex(index)
	if typeof(material) ~= "EnumItem" or material.EnumType ~= Enum.Material then
		error("Unable to cast value to Material", 2)
	end
	slots[index] = { material, tostring(variant or ""), if typeof(color) == "Color3" then color else Color3.new() }
end

function TERRAIN.ResetMaterialSlot(_, index)
	slots[slotIndex(index)] = nil
end

-- Beam ------------------------------------------------------------------

local BEAM = {}
local offsets = {}

-- Where the texture's scroll starts, for the renderer.
function BEAM.SetTextureOffset(beam, offset)
	offsets[host.id(beam)] = offset or 0
end

-- Path2D ----------------------------------------------------------------

local PATH2D = {}
local MAX_POINTS = 100
local points = {}
local SAMPLES = 256

local function pointsOf(path)
	local id = host.id(path)
	points[id] = points[id] or {}
	return points[id]
end

local function changed(path)
	host.fire(host.id(path), "ControlPointChanged")
end

local function checkPoint(point)
	if typeof(point) ~= "Path2DControlPoint" then
		error("Unable to cast value to Path2DControlPoint", 3)
	end
	return point
end

local function checkIndex(method, index, high)
	if typeof(index) ~= "number" or index < 1 or index > high then
		error(string.format("Path2D:%s() at index %s access out of bounds", method, tostring(index)), 3)
	end
	return math.floor(index)
end

function PATH2D.GetMaxControlPoints()
	return MAX_POINTS
end

function PATH2D.GetControlPoints(path)
	return table.clone(pointsOf(path))
end

function PATH2D.GetControlPoint(path, index)
	local list = pointsOf(path)
	return list[checkIndex("GetControlPoint", index, #list)]
end

function PATH2D.SetControlPoints(path, list)
	check(list, "table", 1)
	local copy = {}
	for i, point in list do
		copy[i] = checkPoint(point)
	end
	points[host.id(path)] = copy
	changed(path)
end

function PATH2D.InsertControlPoint(path, index, point)
	local list = pointsOf(path)
	table.insert(list, checkIndex("InsertControlPoint", index, #list + 1), checkPoint(point))
	changed(path)
end

function PATH2D.UpdateControlPoint(path, index, point)
	local list = pointsOf(path)
	list[checkIndex("UpdateControlPoint", index, #list)] = checkPoint(point)
	changed(path)
end

function PATH2D.RemoveControlPoint(path, index)
	local list = pointsOf(path)
	table.remove(list, checkIndex("RemoveControlPoint", index, #list))
	changed(path)
end

-- The parent's size in pixels, which `Scale` parts of positions stand for.
-- A parent laid out nowhere (a ScreenGui outside PlayerGui) still has
-- its own offsets.
local function frameSize(path)
	local parent = path.Parent
	if not isA(parent, "GuiBase2d") then
		return Vector2.zero
	end
	local size = parent.AbsoluteSize
	if size == Vector2.zero and isA(parent, "GuiObject") then
		size = Vector2.new(parent.Size.X.Offset, parent.Size.Y.Offset)
	end
	return size
end

local function pixels(u, size)
	return Vector2.new(u.X.Scale * size.X + u.X.Offset, u.Y.Scale * size.Y + u.Y.Offset)
end

local ZERO = UDim2.new()

-- Every segment as the Bezier control points between two control points
-- in pixels: a tangent left at zero adds no control point, so a segment
-- with no tangents is a straight line and one with a single tangent is
-- quadratic.
local function segments(path, size)
	local list = pointsOf(path)
	local out = {}
	local count = #list
	local last = if path.Closed and count > 1 then count else count - 1
	for i = 1, last do
		local a, b = list[i], list[i % count + 1]
		local p0, p3 = pixels(a.Position, size), pixels(b.Position, size)
		local seg = { p0 }
		if a.RightTangent ~= ZERO then
			table.insert(seg, p0 + pixels(a.RightTangent, size))
		end
		if b.LeftTangent ~= ZERO then
			table.insert(seg, p3 + pixels(b.LeftTangent, size))
		end
		table.insert(seg, p3)
		table.insert(out, seg)
	end
	return out
end

local function bezier(seg, t)
	local work = table.clone(seg)
	for n = #work - 1, 1, -1 do
		for i = 1, n do
			work[i] = work[i]:Lerp(work[i + 1], t)
		end
	end
	return work[1]
end

local function derivative(seg, t)
	local n = #seg - 1
	local diff = {}
	for i = 1, n do
		diff[i] = (seg[i + 1] - seg[i]) * n
	end
	return bezier(diff, t)
end

-- The segment and its own parameter for a parameter over the whole path.
local function locate(segs, t)
	local n = #segs
	local at = math.clamp(t, 0, 1) * n
	local i = math.min(math.floor(at), n - 1)
	return segs[i + 1], at - i
end

-- Cumulative pixel lengths at SAMPLES steps along every segment.
local function arcTable(segs)
	local lengths = { 0 }
	local total = 0
	for _, seg in segs do
		local prev = seg[1]
		for s = 1, SAMPLES do
			local here = bezier(seg, s / SAMPLES)
			total += (here - prev).Magnitude
			prev = here
			table.insert(lengths, total)
		end
	end
	return lengths, total
end

-- The whole-path parameter at fraction `t` of the length.
local function byLength(segs, t)
	local lengths, total = arcTable(segs)
	if total == 0 then
		return math.clamp(t, 0, 1)
	end
	local want = math.clamp(t, 0, 1) * total
	for i = 2, #lengths do
		if lengths[i] >= want then
			local span = lengths[i] - lengths[i - 1]
			local frac = if span > 0 then (want - lengths[i - 1]) / span else 0
			return ((i - 2) + frac) / (#lengths - 1)
		end
	end
	return 1
end

local function curve(path, method, t)
	check(t, "number", 1)
	local size = frameSize(path)
	local segs = segments(path, size)
	if #segs == 0 then
		error(string.format("Path2D:%s() attempting to query with zero segments", method), 3)
	end
	return segs, size
end

-- A point in pixels as the parent's scale, which is how Roblox answers.
local function position(v, size)
	return UDim2.new(if size.X ~= 0 then v.X / size.X else 0, 0, if size.Y ~= 0 then v.Y / size.Y else 0, 0)
end

function PATH2D.GetPositionOnCurve(path, t)
	local segs, size = curve(path, "GetPositionOnCurve", t)
	return position(bezier(locate(segs, t)), size)
end

function PATH2D.GetTangentOnCurve(path, t)
	local segs = curve(path, "GetTangentOnCurve", t)
	return derivative(locate(segs, t))
end

function PATH2D.GetPositionOnCurveArcLength(path, t)
	local segs, size = curve(path, "GetPositionOnCurveArcLength", t)
	return position(bezier(locate(segs, byLength(segs, t))), size)
end

function PATH2D.GetTangentOnCurveArcLength(path, t)
	local segs = curve(path, "GetTangentOnCurveArcLength", t)
	return derivative(locate(segs, byLength(segs, t)))
end

function PATH2D.GetLength(path)
	local _, total = arcTable(segments(path, frameSize(path)))
	return total
end

-- The curve's extent in pixels, widened by the line's thickness.
function PATH2D.GetBoundingRect(path)
	local size = frameSize(path)
	local list = pointsOf(path)
	if #list == 0 then
		return Rect.new(0, 0, 0, 0)
	end
	local first = pixels(list[1].Position, size)
	local minX, minY, maxX, maxY = first.X, first.Y, first.X, first.Y
	for _, seg in segments(path, size) do
		for s = 0, SAMPLES do
			local p = bezier(seg, s / SAMPLES)
			minX, minY = math.min(minX, p.X), math.min(minY, p.Y)
			maxX, maxY = math.max(maxX, p.X), math.max(maxY, p.Y)
		end
	end
	local pad = path.Thickness
	return Rect.new(minX - pad, minY - pad, maxX + pad, maxY + pad)
end

-- Dispatch --------------------------------------------------------------

function M.member(class, id, key)
	local instance = host.instance(id)
	if class == "Workspace" then
		if key == "PersistentLoaded" then
			return host.signal(id, key), true
		end
		local f = WORKSPACE[key]
		if f then
			return f, true
		end
	end
	if instance:IsA("WorldRoot") then
		local f = WORLD_ROOT[key]
		if f then
			return f, true
		end
	end
	if instance:IsA("BasePart") then
		local f = BASE_PART[key]
		if f then
			return f, true
		end
		local read = BASE_PART_PROPS[key]
		if read then
			return read(instance), true
		end
	end
	if class == "Model" or instance:IsA("Model") and not instance:IsA("WorldRoot") then
		local f = MODEL[key]
		if f then
			return f, true
		end
	end
	if class == "Camera" then
		local f = CAMERA[key]
		if f then
			return f, true
		end
	end
	if class == "Terrain" and TERRAIN[key] then
		return TERRAIN[key], true
	end
	if class == "Beam" and BEAM[key] then
		return BEAM[key], true
	end
	if class == "Path2D" then
		if key == "ControlPointChanged" then
			return host.signal(id, key), true
		end
		local f = PATH2D[key]
		if f then
			return f, true
		end
	end
	return nil, false
end

return M
