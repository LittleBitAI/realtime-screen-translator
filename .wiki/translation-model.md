---
scope: project
severity: contract
repeat: rule
triggers: ["번역 ?모델", "gemini", "제미나이", "flash ?lite", "가드레일", "안전 ?설정", "safety", "프리필", "prefill", "LLM", "모델을? (바꾸|교체|변경)"]
slots: []
sources: [README.md]
links: [project, skip-sound-effects]
---

# Translation runs on Gemini 3.1 Flash Lite with safety guardrails off and a prefill

Rule. The translation call uses Gemini 3.1 Flash Lite. Every safety
category the API exposes is set to its least restrictive threshold, and the
request carries a prefill that starts the model's answer in the expected
output shape. Do not swap the model, re-enable a guardrail, or drop the
prefill as a "fix" for a refusal, a blocked response, latency or cost;
raise that with the person instead, since the README fixes all three.

Why. `README.md` names the model and asks for the default guardrails off
"plus a prefill". Manga dialogue routinely contains violence and crude
language, and a blocked response leaves a speech bubble untranslated on
screen.

What goes wrong. A different model changes latency and quality on the
reference phone; a guardrail left at default silently blanks pages; a
missing prefill lets the model answer in prose instead of the format the
overlay parses.
