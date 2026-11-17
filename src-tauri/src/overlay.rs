//! A7: the Windows overlay's page. The overlay is a transparent,
//! click-through Tauri window over the whole primary monitor, left out of
//! capture (`app.rs`). Its page is this inline one, not the app's frontend,
//! and Rust draws on it by running [`script`] in it: each placement (R28) is
//! a white shape with its corner radius and the Korean inside, at the
//! placement's screen pixels. Like the Android overlay (`Overlay.kt`), the
//! text is wrapped by character and kept at 1.2 lines apart, and shrunk
//! while the font on the device draws it taller than the shape holds.

/// The page. `PAD` is `layout::PAD`; `MIN_TEXT_PX` is the smallest size
/// the Android overlay shrinks the text to.
pub const HTML: &str = r#"<!doctype html><meta charset=utf-8>
<style>
html,body{margin:0;width:100%;height:100%;overflow:hidden;background:transparent}
.s{position:absolute;box-sizing:border-box;background:white;color:black;overflow:hidden;
display:flex;align-items:center;justify-content:center}
.s span{display:block;width:100%;text-align:center;line-height:1.2;white-space:pre-wrap;word-break:break-all;
font-family:'Malgun Gothic','Noto Sans KR',sans-serif}
</style>
<body><script>
var PAD=4,MIN_TEXT_PX=6;
function draw(list){
  var r=window.devicePixelRatio||1;
  document.body.replaceChildren();
  list.forEach(function(p){
    var d=document.createElement('div'),t=document.createElement('span');
    d.className='s';
    d.style.left=(p.x/r)+'px';d.style.top=(p.y/r)+'px';
    d.style.width=(p.w/r)+'px';d.style.height=(p.h/r)+'px';
    d.style.borderRadius=(p.radius/r)+'px';d.style.padding=(PAD/r)+'px';
    t.textContent=p.ko;d.appendChild(t);document.body.appendChild(d);
    var size=p.font_px,room=Math.max(1,p.h-2*PAD)/r;
    t.style.fontSize=(size/r)+'px';
    while(t.offsetHeight>room&&size>MIN_TEXT_PX){size-=1;t.style.fontSize=(size/r)+'px';}
  });
}
</script>"#;

/// [`HTML`] as a `data:` URL the overlay window loads.
pub fn data_url() -> String {
    let mut out = String::from("data:text/html;charset=utf-8,");
    for b in HTML.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The script that shows exactly `placements` (a JSON array of
/// `PlacementOut`, screen pixels) on the page; anything else clears it.
pub fn script(placements: &serde_json::Value) -> String {
    let list = if placements.is_array() { placements.to_string() } else { "[]".into() };
    format!("draw({list})")
}
