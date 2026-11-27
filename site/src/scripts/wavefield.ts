// Background field: waves behind the hero (active, follows the demo's voice) and the Get Eqho band (calm),
// echo rings behind the footer (calm). WebGL1, one full-screen triangle, opaque, base colour = band colour.
// Host: data-wf="active" | "calm" | "rings".

// Port of eqho-core pillfx.rs VoiceFollower.
class VoiceFollower {
  loud = 0; tone = 0.5; phase = 0;
  step(dt: number, target: number, targetTone: number) {
    // Fed every frame here, so the 0.30 s stale rule of the app never triggers.
    const tau = target > this.loud ? 0.18 : 0.50;
    this.loud += (target - this.loud) * (1 - Math.exp(-dt / tau));
    if (this.loud > 0.08) this.tone += (targetTone - this.tone) * (1 - Math.exp(-dt / 0.30));
    this.phase += dt * (0.5 + 1.7 * this.loud);
  }
}

/** Speech envelope while the demo speaks (t = seconds since speaking started). */
export function speech(t: number): { loud: number; tone: number } {
  const syl = Math.pow(0.5 + 0.5 * Math.sin(2 * Math.PI * 4.2 * t), 1.6);
  const u = (t % 2.6) / 2.6;
  const shape = Math.sqrt(Math.max(0, Math.sin(Math.PI * u)));
  return { loud: (0.25 + 0.75 * syl) * shape * (0.8 + 0.2 * Math.sin(1.3 * t)), tone: 0.5 + 0.3 * Math.sin(0.9 * t) };
}

const VS = 'attribute vec2 a;void main(){gl_Position=vec4(a,0.,1.);}';
const FS = `precision mediump float;
uniform vec2 uRes;uniform vec3 uBase;uniform float uT,uLoud,uTone,uPhase,uMode,uLineY,uFocusX,uGain;uniform vec2 uC;
const float PI=3.14159265;
float gs(float x,float s){return exp(-x*x/(2.0*s*s));}
float mw(float xn,float k,float p){return (sin(xn*PI*1.10*k+p)+0.45*sin(xn*PI*1.85*k-1.3*p+0.8)+0.20*sin(xn*PI*2.70*k+0.7*p+2.1))/1.65;}
float sw(float xn,float k,float p){return (sin(xn*PI*1.30*k+0.85*p+1.9)+0.40*sin(xn*PI*2.05*k-1.1*p+3.0))/1.40;}
vec3 ln(float d,float th){float fr=0.35*th;return vec3(gs(d-fr,th),gs(d,th),gs(d+fr,th));}
void main(){
  vec2 fc=gl_FragCoord.xy;float y=fc.y/uRes.y;float x=fc.x/uRes.y;float u=fc.x/uRes.x;float xn=u*2.0-1.0;
  float e=smoothstep(0.0,1.0,uLoud);float amp=0.05+0.50*e;float k=1.0+0.55*uTone;float p=uPhase;
  float taper=pow(clamp(sin((xn+1.0)*PI/2.0),0.0,1.0),0.8);
  vec3 W=vec3(0.25,0.55,1.0);vec3 col=vec3(0.0);
  float focus=mix(0.15,1.0,gs(u-uFocusX,0.26));
  float lift=0.6+0.4*e;
  if(uMode<0.5){
    float S=0.12;float th=0.011;
    float y1=uLineY+mw(xn,k,p)*amp*S*taper;
    float y2=uLineY+0.05+sw(xn,k,p)*amp*0.6*S*taper;
    float y3=uLineY-0.05+mw(xn,k,p+2.4)*amp*0.35*S*taper;
    vec3 g=ln(y-y1,th)+0.55*ln(y-y2,th)+0.35*ln(y-y3,th);
    col+=W*g*focus*taper*lift;
    float wx=x+0.22*sin(1.7*y+0.33*uT);float wy=y+0.22*sin(1.9*x+0.27*uT);
    float drift=0.5+0.5*sin(2.1*wx+2.6*wy+0.21*uT);
    col+=mix(vec3(0.035,0.06,0.14),vec3(0.08,0.17,0.42),drift*0.55)*0.9*gs(y-uLineY,0.10)*focus;
  } else {
    vec2 q=(vec2(x,y)-uC)*vec2(0.55,1.0);float d=length(q);
    for(int i=0;i<5;i++){
      float ph=fract(0.05*uT+float(i)/5.0);float r=ph*1.6;float th=0.008+0.02*ph;
      float f=(1.0-ph)*(1.0-ph)*smoothstep(0.0,0.12,ph)*(0.15+0.85*uLoud);
      col+=W*ln(d-r,th)*f;
    }
  }
  col*=uGain;
  col=1.0-exp(-col*1.25);
  gl_FragColor=vec4(uBase+col,1.0);
}`;

interface Field {
  canvas: HTMLCanvasElement; host: HTMLElement; gl: WebGLRenderingContext; u: Record<string, WebGLUniformLocation | null>;
  calm: boolean; rings: boolean; visible: boolean; f: VoiceFollower; base: [number, number, number];
}

function hexToRgb(c: string): [number, number, number] {
  const m = c.trim().match(/^#?([0-9a-f]{6})$/i);
  if (m) { const n = parseInt(m[1], 16); return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]; }
  const r = c.match(/\d+(\.\d+)?/g);
  return r ? [Number(r[0]) / 255, Number(r[1]) / 255, Number(r[2]) / 255] : [0, 0, 0];
}

// No WebGL: a soft CSS glow for the wave bands, a plain footer.
const fallback = (h: HTMLElement) => { if (h.dataset.wf !== 'rings') h.classList.add('wf-fallback'); };

function init() {
  const hosts = Array.from(document.querySelectorAll<HTMLElement>('[data-wf]'));
  if (!hosts.length) return;
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const fields: Field[] = [];

  for (const host of hosts) {
    const canvas = host.querySelector('canvas')!;
    const gl = canvas.getContext('webgl', { alpha: false, antialias: false, premultipliedAlpha: false, powerPreference: 'low-power' }) as WebGLRenderingContext | null;
    if (!gl) { fallback(host); continue; }
    const sh = (type: number, src: string) => { const s = gl.createShader(type)!; gl.shaderSource(s, src); gl.compileShader(s); return s; };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS)); gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS)); gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) { fallback(host); continue; }
    gl.useProgram(prog);
    const buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, 'a'); gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const u: Field['u'] = {};
    for (const n of ['uRes', 'uBase', 'uT', 'uLoud', 'uTone', 'uPhase', 'uMode', 'uLineY', 'uFocusX', 'uGain', 'uC']) u[n] = gl.getUniformLocation(prog, n);
    host.classList.add('wf-on');
    fields.push({ canvas, host, gl, u, calm: host.dataset.wf !== 'active', rings: host.dataset.wf === 'rings', visible: true, f: new VoiceFollower(), base: [0, 0, 0] });
  }
  if (!fields.length) return;

  const readBase = () => { for (const fd of fields) fd.base = hexToRgb(getComputedStyle(fd.host).backgroundColor); };
  readBase();
  document.addEventListener('eqho-theme', () => { readBase(); if (reduced.matches) frame(performance.now(), true); });

  let changed = false;
  const resize = () => {
    changed = false;
    for (const fd of fields) {
      const r = fd.canvas.getBoundingClientRect();
      const s = 0.5 * Math.min(window.devicePixelRatio || 1, 1.5);
      const w = Math.max(2, Math.round(r.width * s)), h = Math.max(2, Math.round(r.height * s));
      if (fd.canvas.width !== w || fd.canvas.height !== h) { fd.canvas.width = w; fd.canvas.height = h; changed = true; }
    }
    return changed;
  };
  // A resized canvas is cleared: redraw every field at once so it never flashes black.
  const redraw = () => { if (reduced.matches) frame(performance.now(), true); else for (const fd of fields) draw(fd, performance.now()); };
  new ResizeObserver(() => { if (resize()) redraw(); }).observe(document.documentElement);

  const io = new IntersectionObserver((es) => { for (const e of es) { const fd = fields.find((x) => x.host === e.target); if (fd) fd.visible = e.isIntersecting; } kick(); });
  for (const fd of fields) io.observe(fd.host);

  // Geometry from the DOM: line height (stage's lower third), focus x, echo centre (the pill).
  const geom = (fd: Field) => {
    const hr = fd.canvas.getBoundingClientRect();
    const anchor = fd.host.querySelector<HTMLElement>('[data-wf-anchor]');
    const pill = fd.host.querySelector<HTMLElement>('[data-wf-centre]');
    let lineY = fd.calm ? 0.16 : 0.3, focusX = fd.calm ? 0.62 : 0.72, c: [number, number] = [hr.width / hr.height * 0.72, 0.4];
    if (anchor && hr.height > 0) {
      const a = anchor.getBoundingClientRect();
      lineY = 1 - (a.top + a.height * 0.78 - hr.top) / hr.height;
      focusX = (a.left + a.width / 2 - hr.left) / hr.width;
    }
    if (pill && hr.height > 0) {
      const p = pill.getBoundingClientRect();
      c = [(p.left + p.width / 2 - hr.left) / hr.height, 1 - (p.top + p.height / 2 - hr.top) / hr.height];
    } else if (anchor && hr.height > 0) {
      c = [focusX * hr.width / hr.height, lineY];
    }
    return { lineY, focusX, c };
  };

  const t0 = performance.now();
  let lastDraw = 0, raf = 0;
  const draw = (fd: Field, now: number) => {
    const { gl, u } = fd;
    const t = (now - t0) / 1000;
    const g = geom(fd);
    gl.viewport(0, 0, fd.canvas.width, fd.canvas.height);
    gl.uniform2f(u.uRes, fd.canvas.width, fd.canvas.height);
    gl.uniform3f(u.uBase, fd.base[0], fd.base[1], fd.base[2]);
    gl.uniform1f(u.uT, fd.calm ? t * 0.6 : t);
    gl.uniform1f(u.uLoud, fd.f.loud); gl.uniform1f(u.uTone, fd.f.tone); gl.uniform1f(u.uPhase, fd.f.phase);
    gl.uniform1f(u.uMode, fd.rings ? 1 : 0);
    gl.uniform1f(u.uLineY, g.lineY); gl.uniform1f(u.uFocusX, g.focusX);
    gl.uniform1f(u.uGain, fd.rings ? 0.22 : fd.calm ? 0.28 : 0.38);
    gl.uniform2f(u.uC, g.c[0], g.c[1]);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
  const feed = (fd: Field, dt: number) => {
    if (fd.calm) { fd.f.step(dt * 0.6, 0.12, 0.5); return; }
    const demo = (window as any).EqhoDemo as { speakingAt?: () => number | null } | undefined;
    const st = demo?.speakingAt?.() ?? null;
    if (st === null) fd.f.step(dt, 0, fd.f.tone);
    else { const s = speech(st); fd.f.step(dt, s.loud, s.tone); }
  };
  function frame(now: number, once = false) {
    raf = 0;
    if (once) {
      for (const fd of fields) {
        if (!fd.calm) { fd.f.loud = 0.55; fd.f.tone = 0.5; fd.f.phase = 1.2; } else { fd.f.loud = 0.12; fd.f.phase = 0.6; }
        draw(fd, t0 + 4000);
      }
      return;
    }
    const any = !document.hidden && fields.some((f) => f.visible);
    if (!any) return;
    if (now - lastDraw >= 1000 / 30 - 2) {
      const ddt = Math.min(0.1, (now - lastDraw) / 1000); lastDraw = now;
      for (const fd of fields) { feed(fd, ddt); if (fd.visible) draw(fd, now); }
    }
    raf = requestAnimationFrame((n) => frame(n));
  }
  function kick() {
    if (reduced.matches) { frame(performance.now(), true); return; }
    if (!raf && !document.hidden) { lastDraw = performance.now(); raf = requestAnimationFrame((n) => frame(n)); }
  }
  resize(); redraw();
  document.addEventListener('visibilitychange', kick);
  reduced.addEventListener?.('change', kick);
  kick();
}

init();
