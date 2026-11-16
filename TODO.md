# Suja TODO

Plan: ~/.claude/plans/create-a-plan-to-soft-kettle.md

- [x] M0 Scaffold: workspace, lints, suja-proto, CLI skeleton
- [x] M1 Text agent loop: 28/32 on eval/toolcalls.ron (prompt tuned on these cases; checks tool name only; the model sometimes claims it ran a command it never called), first token about 70 to 200 ms, peak VRAM 3536 MiB (2026-11-13)
- [~] M2 Speech in: streaming zipformer + silero on CPU, capture through crownui audio; wav transcription works across reuse; accuracy gate on real voice not done, 20M model misses sentence starts on synthetic audio
- [~] M3 Speech out: Pocket int8 through sherpa-onnx (shared lib) with clause chunking, PipeWire playback; first audio of `say` 282 ms; Kokoro needs a lexicon, Kitten untested; Pocket ONNX export is non-commercial
- [~] M4 Full pipeline: sujad runs; `suja status|say|ask|talk|watch`; typed turns, tool turns, follow-ups and the confirm-with-yes flow verified; evdev hold-to-talk wired but a real voice turn is untested
- [~] M5 Screen context: clipboard preview reaches the model (verified); primary selection via wl-paste wired; focused window stays None because crownpositor never emits the plugin-host window events its XML declares
- [~] M6 crownbar orb widget: `assistant` widget, orb icon, transcript panel; compiles and lints; bar not run with it; auto-open bubble and transcript row not done
- [~] M7 Packaging: PKGBUILD, sujad.service, preset, fetch-models.sh, suja.ron.example written; makepkg and a release build not run
