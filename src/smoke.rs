//! Native input-path verification, enabled only by the explicit --smoke-test flag.
use crate::{
    app::Romo,
    geometry::{Pos2, pos2, vec2},
    model::Kind,
    storage,
};
use gpui::{App, AppContext, Keystroke, WindowHandle};
use std::time::Duration;

fn key(handle: WindowHandle<Romo>, cx: &mut App, key: &str) -> anyhow::Result<()> {
    let key = Keystroke::parse(key)?;
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_keystroke(key, cx);
    })?;
    Ok(())
}
fn mouse(handle: WindowHandle<Romo>, cx: &mut App, start: Pos2, end: Pos2) -> anyhow::Result<()> {
    handle.update(cx, |app, window, cx| {
        app.smoke_mouse(start, end, window, cx)
    })?;
    Ok(())
}

pub fn run(handle: WindowHandle<Romo>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut note_center=Pos2::ZERO;
        let mut original_pos=Pos2::ZERO;
        let mut shape_center=Pos2::ZERO;
        let mut screen_anchor=Pos2::ZERO;
        let result: anyhow::Result<()> = async {
            for step in 0..60 {
                gpui::Timer::after(Duration::from_millis(if matches!(step, 21 | 22 | 25 | 38 | 42 | 46 | 47 | 48 | 49) { 800 } else { 180 })).await;
                cx.update(|cx| -> anyhow::Result<()> {
                    // Synthetic input bypasses the platform's frame request. Flush a real
                    // GPUI layout/paint pass before observing state, also on headless X11.
                    cx.update_window(handle.into(), |_, window, cx| { window.draw(cx).clear(); })?;
                    let (document,canvas,editing)=handle.read(cx)?.smoke_snapshot();
                    let (declutter,selected,inline)=handle.read(cx)?.smoke_ui();
                    match step {
                        0=>key(handle,cx,"ctrl-n")?,
                        1=>{anyhow::ensure!(document.board.items.is_empty(),"New board shortcut failed");key(handle,cx,"n")?;},
                        2=>{note_center=canvas.center()-vec2(220.0,0.0);mouse(handle,cx,note_center,note_center)?;},
                        3=>{
                            anyhow::ensure!(editing&&inline&&document.board.items.len()==1,"Sticky creation or inline editor failed");
                            for c in "A native thought — café".chars(){key(handle,cx,&c.to_string())?;}
                        }
                        4=>{anyhow::ensure!(document.board.items[0].text=="A native thought — café","Text is not saved live while editing");key(handle,cx,"ctrl-enter")?;},
                        5=>{
                            anyhow::ensure!(!editing&&document.board.items[0].text=="A native thought — café","Native text input or apply failed");
                            original_pos=document.board.items[0].pos;
                            mouse(handle,cx,note_center,note_center+vec2(100.0,60.0))?;
                        }
                        6=>{
                            let expected=original_pos+vec2(100.0,60.0)/document.view.zoom;
                            anyhow::ensure!(document.board.items[0].pos.distance(expected)<0.1,"Dragging a note failed");
                            note_center+=vec2(100.0,60.0);key(handle,cx,"ctrl-d")?;
                        }
                        7=>{anyhow::ensure!(document.board.items.len()==2,"Duplicate failed");key(handle,cx,"delete")?;},
                        8=>{anyhow::ensure!(document.board.items.len()==1,"Delete failed");key(handle,cx,"ctrl-z")?;},
                        9=>{anyhow::ensure!(document.board.items.len()==2,"Undo failed");key(handle,cx,"ctrl-shift-z")?;},
                        10=>{anyhow::ensure!(document.board.items.len()==1,"Redo failed");key(handle,cx,"r")?;},
                        11=>{shape_center=canvas.center()+vec2(150.0,60.0);mouse(handle,cx,shape_center,shape_center)?;},
                        12=>{anyhow::ensure!(document.board.items.iter().any(|i|i.kind==Kind::Rectangle),"Shape creation failed");key(handle,cx,"escape")?;},
                        13=>key(handle,cx,"l")?,
                        14=>mouse(handle,cx,note_center,shape_center)?,
                        15=>{
                            let arrow=document.board.items.iter().find(|i|i.kind==Kind::Arrow).ok_or_else(||anyhow::anyhow!("Connector creation failed"))?;
                            anyhow::ensure!(arrow.start_anchor.is_some()&&arrow.end_anchor.is_some(),"Connector attachment failed");key(handle,cx,"p")?;
                        }
                        16=>{let start=canvas.center()+vec2(-100.0,200.0);mouse(handle,cx,start,start+vec2(140.0,40.0))?;},
                        17=>{
                            anyhow::ensure!(document.board.items.iter().any(|i|i.kind==Kind::Pen),"Freehand drawing failed");
                            anyhow::ensure!(selected==0,"A new drawing is selected by default");
                            document.validate().map_err(anyhow::Error::msg)?;
                            let folder=std::env::temp_dir().join(format!("romo-native-check-{}",std::process::id()));
                            let path=folder.join("board.romo");
                            storage::save(&path,&document).map_err(anyhow::Error::msg)?;
                            let reopened=storage::load(&path).map_err(anyhow::Error::msg)?;
                            anyhow::ensure!(reopened.board==document.board,"Saved board round trip failed");
                            std::fs::remove_dir_all(folder)?;
                            key(handle,cx,"v")?;
                        }
                        18=>{
                            let item=&document.board.items[0];let bottom=document.view.to_screen(item.rect().max,canvas);
                            mouse(handle,cx,note_center,note_center)?;
                            mouse(handle,cx,bottom,bottom+vec2(40.0,30.0))?;
                        }
                        19=>{anyhow::ensure!(document.board.items[0].size.x>190.0&&document.board.items[0].size.y>190.0,"Resizing failed");key(handle,cx,"2")?;},
                        20=>{anyhow::ensure!(document.view.zoom.is_finite(),"Fit canvas failed");screen_anchor=document.view.to_screen(document.board.items[0].pos,canvas);key(handle,cx,"y")?;},
                        21=>{anyhow::ensure!(declutter&&canvas.min.y==0.0,"Declutter failed or the board does not fill the window: declutter={declutter}, canvas={canvas:?}");anyhow::ensure!(document.view.to_screen(document.board.items[0].pos,canvas).distance(screen_anchor)<0.01,"Declutter shifted the board");println!("Declutter frame ready");},
                        22=>{key(handle,cx,"y")?;key(handle,cx,"t")?;},
                        23=>{anyhow::ensure!(!declutter&&canvas.min.y==0.0,"Declutter did not restore controls, or controls took board space");anyhow::ensure!(document.view.to_screen(document.board.items[0].pos,canvas).distance(screen_anchor)<0.01,"Restoring controls shifted the board");let point=canvas.center()+vec2(-200.0,-200.0);mouse(handle,cx,point,point)?;},
                        24=>{anyhow::ensure!(inline&&editing,"Adding text did not start live canvas editing");for c in "Live y text".chars(){key(handle,cx,&c.to_string())?;}println!("Inline frame ready");},
                        25=>{let text=document.board.items.iter().find(|i|i.kind==Kind::Text).unwrap();anyhow::ensure!(text.text=="Live y text"&&!declutter&&inline,"Typing Y toggled declutter or text did not update live");key(handle,cx,"enter")?;for c in "Second line".chars(){key(handle,cx,&c.to_string())?;}key(handle,cx,"enter")?;for c in "Third line".chars(){key(handle,cx,&c.to_string())?;}},
                        26=>{let text=document.board.items.iter().find(|i|i.kind==Kind::Text).unwrap();anyhow::ensure!(text.text=="Live y text\nSecond line\nThird line"&&text.size.y>=text.font_size*1.35*3.0,"Multiline editing did not expand the text object");let point=canvas.center()+vec2(300.0,-200.0);mouse(handle,cx,point,point)?;},
                        27=>{anyhow::ensure!(!editing&&document.board.items.iter().any(|i|i.kind==Kind::Text&&!i.text.is_empty()),"Click-away discarded live text");key(handle,cx,"ctrl-z")?;},
                        28=>{anyhow::ensure!(document.board.items.iter().find(|i|i.kind==Kind::Text).unwrap().text.is_empty(),"Live text edits are not grouped for undo");key(handle,cx,"ctrl-shift-z")?;},
                        29=>{anyhow::ensure!(document.board.items.iter().find(|i|i.kind==Kind::Text).unwrap().text.contains("Third line"),"Live text redo failed");key(handle,cx,"b")?;},
                        30=>{anyhow::ensure!(handle.read(cx)?.smoke_tool()=="Pen","B did not choose the pen");let point=canvas.center()+vec2(200.0,220.0);mouse(handle,cx,point,point)?;},
                        31=>{let dot=document.board.items.last().unwrap();anyhow::ensure!(dot.kind==Kind::Pen&&dot.rect().width()==0.0&&selected==0,"Pen taps did not create unselected dots");document.validate().map_err(anyhow::Error::msg)?;key(handle,cx,"y")?;},
                        32=>{anyhow::ensure!(declutter,"Declutter failed while using pen");let point=canvas.center()+vec2(100.0,-220.0);mouse(handle,cx,point,point+vec2(40.0,20.0))?;},
                        33=>{anyhow::ensure!(declutter&&selected==0&&document.board.items.last().unwrap().kind==Kind::Pen,"Drawing in declutter selected the stroke or failed");key(handle,cx,"y")?;},
                        34=>{anyhow::ensure!(!declutter,"Could not leave declutter after drawing");},
                        35=>key(handle,cx,"v")?,
                        36=>{
                            let stroke=document.board.items.iter().find(|i|i.kind==Kind::Pen&&i.rect().width()>100.0).unwrap();
                            let point=document.view.to_screen(stroke.rect().center(),canvas);
                            original_pos=stroke.pos;
                            mouse(handle,cx,point,point)?;
                        },
                        37=>{
                            anyhow::ensure!(selected==1,"Could not select the freehand stroke");
                            let stroke=document.board.items.iter().find(|i|i.kind==Kind::Pen&&i.rect().width()>100.0).unwrap();
                            let corner=stroke.rect().min+vec2(stroke.rect().width()*0.75,stroke.rect().height()*0.1);
                            anyhow::ensure!(document.board.hit_test(corner,6.0/document.view.zoom,false)!=Some(stroke.id),"Regression pointer hits the stroke itself");
                            let point=document.view.to_screen(corner,canvas);
                            mouse(handle,cx,point,point+vec2(50.0,-30.0))?;
                            key(handle,cx,"y")?;
                        },
                        38=>{
                            let stroke=document.board.items.iter().find(|i|i.kind==Kind::Pen&&i.rect().width()>100.0).unwrap();
                            anyhow::ensure!(stroke.pos.distance(original_pos+vec2(50.0,-30.0)/document.view.zoom)<0.01&&declutter&&selected==1,"Dragging the empty selection rectangle failed or declutter removed selection");
                            println!("Selection frame ready");
                        },
                        39=>{key(handle,cx,"y")?;key(handle,cx,"escape")?;},
                        40=>{
                            let image=image::RgbaImage::from_fn(200,120,|x,y|image::Rgba([((x*255)/200) as u8,((y*255)/120) as u8,150,255]));
                            let mut bytes=std::io::Cursor::new(Vec::new());
                            image.write_to(&mut bytes,image::ImageFormat::Png)?;
                            cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(gpui::ImageFormat::Png,bytes.into_inner())));
                        },
                        41=>key(handle,cx,"ctrl-v")?,
                        42=>{
                            anyhow::ensure!(document.board.items.last().is_some_and(|i|i.kind==Kind::Image&&i.image.as_ref().is_some_and(|a|a.width==200&&a.height==120)),"Native image clipboard paste failed");
                            println!("Image frame ready");
                            key(handle,cx,"ctrl-c")?;
                            println!("External image copied");
                        },
                        43=>key(handle,cx,"ctrl-v")?,
                        44=>{
                            let images:Vec<_>=document.board.items.iter().filter(|i|i.kind==Kind::Image).collect();
                            anyhow::ensure!(images.len()==2&&images[0].size==images[1].size&&images[0].pos!=images[1].pos,"Image copy/paste lost size or duplicated at same position");
                            let restored:storage::Document=serde_json::from_str(&serde_json::to_string(&document)?)?;
                            restored.validate().map_err(anyhow::Error::msg)?;
                            anyhow::ensure!(restored.board==document.board,"Image persistence failed");
                            key(handle,cx,"escape")?;
                        },
                        45=>{
                            if std::env::var_os("ROMO_NATIVE_POINTER_CHECK").is_some(){println!("Control frame ready");}
                        },
                        46=>{
                            if std::env::var_os("ROMO_NATIVE_POINTER_CHECK").is_some(){
                                let (map,gesture,_)=handle.read(cx)?.smoke_interaction();
                                anyhow::ensure!(map&&!gesture&&selected==0,"Minimap button leaked mouse-down to canvas");
                                println!("Control check passed");
                                let item=document.board.items.iter().find(|i|i.kind==Kind::Rectangle).unwrap();
                                let point=document.view.to_screen(item.rect().max-vec2(20.0,20.0),canvas);
                                println!("Context pointer {} {}",point.x,point.y);
                            }
                        },
                        47=>{
                            if std::env::var_os("ROMO_NATIVE_POINTER_CHECK").is_some(){
                                let (_,gesture,context)=handle.read(cx)?.smoke_interaction();
                                anyhow::ensure!(context&&!gesture&&selected==1,"Right-click context menu failed");
                                println!("Context frame ready");
                            }
                        },
                        48=>{handle.update(cx,|app,window,cx|app.smoke_library_check(window,cx))??;println!("Board library, import preservation, restart selection and persisted snap opt-out passed");},
                        49=>{handle.update(cx,|app,window,cx|app.smoke_snap_check(window,cx))??;println!("Object snapping and disabled snapping passed through the native drag handlers");},
                        50=>{handle.update(cx,|app,window,cx|app.smoke_drawing_snap_check(window,cx))??;println!("Snapping skipped a lone stroke and aligned to a whole drawing");key(handle,cx,"e")?;},
                        51=>{anyhow::ensure!(handle.read(cx)?.smoke_tool()=="Eraser","E did not choose the eraser");let from=document.view.to_screen(pos2(40.0,-200.0),canvas);mouse(handle,cx,from,from+vec2(0.0,100.0))?;},
                        52=>{anyhow::ensure!(document.board.items.len()==3&&document.board.items.iter().filter(|i|i.kind==Kind::Pen).count()==2,"Eraser did not remove exactly the swept stroke");key(handle,cx,"ctrl-z")?;},
                        53=>{
                            anyhow::ensure!(document.board.items.iter().filter(|i|i.kind==Kind::Pen).count()==3,"Undo did not restore the erased stroke");
                            key(handle,cx,"v")?;
                            let note=document.board.items.iter().find(|i|i.kind==Kind::Note).unwrap();
                            let point=document.view.to_screen(note.rect().center(),canvas);
                            mouse(handle,cx,point,point)?;
                        }
                        54=>{anyhow::ensure!(selected==1,"Could not select the note");key(handle,cx,"x")?;},
                        55=>{anyhow::ensure!(document.board.items.len()==3&&!document.board.items.iter().any(|i|i.kind==Kind::Note),"X did not delete the selected note");println!("Eraser shortcut, erasing undo and X deletion passed");},
                        56=>{handle.update(cx,|app,window,cx|app.smoke_scale_check(window,cx))??;println!("Drawings scale from the handle, keeping stroke width when enlarged");},
                        57=>{handle.update(cx,|app,window,cx|app.smoke_pen_check(window,cx))??;println!("A tablet pen pressed a toolbar button and drew on the board");},
                        58=>{handle.update(cx,|app,window,cx|app.smoke_text_check(window,cx))??;println!("Clicked text hugs its words, empty text leaves no trace, erasing leaves a trail");},
                        59=>{handle.update(cx,|app,window,cx|app.smoke_paste_check(window,cx))??;println!("Pasted objects land under the pointer and cascade in place");},
                        _=>{}
                    }
                    Ok(())
                })??;
            }
            Ok(())
        }.await;
        match result {
            Ok(())=>{println!("Native smoke test passed: image clipboard and persistence, selection-rectangle drag, board library switching and restart, declutter with stable board position, live multiline Unicode text, click-away, grouped undo/redo, unselected pen strokes and dots, drawing in declutter, connectors, resize, save/reopen, drawing-aware snapping, eraser and X deletion, drawing scaling, tablet pens, fitted text, eraser trail, pasting at the pointer.");let _=cx.update(|cx|cx.quit());}
            Err(error)=>{eprintln!("Native smoke test failed: {error:#}");std::process::exit(1);}
        }
    }).detach();
}
