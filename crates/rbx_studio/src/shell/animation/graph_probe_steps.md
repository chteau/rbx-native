# GraphProbe: what to do in Studio

You only press Run and copy text. Nothing gets uploaded.

1. Open `GraphProbe.rbxl` in Roblox Studio (File > Open from File).
2. Open the Output window: **View** tab > **Output**.
3. Check that Workspace is in Automatic mode: in the Explorer, click **Workspace**. In Properties, **AuthorityMode** should read **Automatic** (the place is saved that way). If it reads Server, change it to Automatic.
4. Press **Run** (F8). Play (F5) also works. You will see a rig in the middle of the baseplate whose right arm moves.
5. Wait about 60 seconds, until the Output shows a line `PROBE done`.
6. In the Output, select everything (click in it, Ctrl+A), copy (Ctrl+C), and paste it back to me. Lines that start with `PROBE` are the ones that matter, but red error lines help too.
7. Press **Stop** (Shift+F5).

## Only if the Output says `REGISTER-FAILED` or `LOAD-FAILED` for a graph

That means Studio will not play a graph that has not been published. To work around it:

1. Open the graph: **Avatar** tab > **Graph Editor**. Select the rig `ProbeRig` and load the graph named in the failed line, for example `ProbeBlend2D`.
2. Publish it from the Graph Editor and copy the asset id it gives you (a number).
3. In the Explorer, open **ServerScriptService** and click **GraphProbe**. In Properties, scroll to **Attributes**, click **+**, and add a Number attribute named `<graph name>_AssetId`, for example `ProbeBlend2D_AssetId`, holding that id.
4. Run again (step 4 above) and send the Output.

Graphs in the place: `ProbeSanity`, `ProbeRandom42`, `ProbeRandom7`, `ProbeBlend2D`, `ProbeOver`.
