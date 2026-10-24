"""Independent original-operator anchors for the selected simple-font gold.

This intentionally limited auditor supports explicit simple-font widths and PDF
text/graphics matrices. It is not a general PDF text-position implementation.
"""
from collections import Counter
from pypdf.generic import ContentStream,TextStringObject,ByteStringObject

def mul(a,b):
 x,y,z,w,e,f=a;g,h,i,j,k,l=b
 return (x*g+z*h,y*g+w*h,x*i+z*j,y*i+w*j,x*k+z*l+e,y*k+w*l+f)
def point(m,x,y):return (m[0]*x+m[2]*y+m[4],m[1]*x+m[3]*y+m[5])
IDENTITY=(1.,0.,0.,1.,0.,0.)
def declarations(reader,page):
 scopes=[]
 def visit(content,res,state=None,context=(),active=None,depth=0):
  assert depth<32
  state=dict(state or dict(ctm=IDENTITY,font=None,size=0.,char=0.,word=0.,scale=1.,rise=0.,leading=0.));gs=[];marks=[];tm=lm=IDENTITY
  def show(value):
   nonlocal tm
   raw=bytes(value) if isinstance(value,ByteStringObject) else value.original_bytes
   font=state['font'];assert font is not None
   assert font['/Subtype'] in ['/Type1','/TrueType','/MMType1'],font['/Subtype']
   assert '/FirstChar' in font and '/Widths' in font, 'explicit source widths required'
   first=int(font['/FirstChar']);widths=font['/Widths'];descriptor=font.get('/FontDescriptor',{})
   for code in raw:
    if active is not None:
     scopes[active]['anchors'].append(point(mul(state['ctm'],tm),0.,state['rise']))
    if 0<=code-first<len(widths):width=float(widths[code-first])
    else:
     assert '/MissingWidth' in descriptor, 'unknown source glyph advance'
     width=float(descriptor['/MissingWidth'])
    advance=(width/1000*state['size']+state['char']+(state['word'] if code==32 else 0))*state['scale']
    tm=mul(tm,(1.,0.,0.,1.,advance,0.))
  for a,op in ContentStream(content,reader).operations:
   if op==b'q':gs.append(state.copy())
   elif op==b'Q':state=gs.pop()
   elif op==b'cm':state['ctm']=mul(state['ctm'],tuple(map(float,a)))
   elif op==b'BT':tm=lm=IDENTITY
   elif op==b'Tm':tm=lm=tuple(map(float,a))
   elif op in [b'Td',b'TD']:
    if op==b'TD':state['leading']=-float(a[1])
    lm=mul(lm,(1.,0.,0.,1.,float(a[0]),float(a[1])));tm=lm
   elif op==b'T*':lm=mul(lm,(1.,0.,0.,1.,0.,-state['leading']));tm=lm
   elif op==b'Tf':state['font']=res['/Font'][a[0]].get_object();state['size']=float(a[1])
   elif op in [b'Tc',b'Tw',b'Tz',b'Ts',b'TL']:
    k={b'Tc':'char',b'Tw':'word',b'Tz':'scale',b'Ts':'rise',b'TL':'leading'}[op];state[k]=float(a[0])/(100 if op==b'Tz' else 1)
   elif op==b'gs':
    d=res['/ExtGState'][a[0]].get_object()
    if '/Font'in d:state['font']=d['/Font'][0].get_object();state['size']=float(d['/Font'][1])
   elif op==b'Tj':show(a[0])
   elif op in [b"'",b'"']:
    if op==b'"':state['word']=float(a[0]);state['char']=float(a[1])
    lm=mul(lm,(1.,0.,0.,1.,0.,-state['leading']));tm=lm;show(a[-1])
   elif op==b'TJ':
    for v in a[0]:
     if isinstance(v,(TextStringObject,ByteStringObject)):show(v)
     else:tm=mul(tm,(1.,0.,0.,1.,-float(v)/1000*state['size']*state['scale'],0.))
   elif op in [b'BMC',b'BDC']:
    marks.append((context,active))
    if op==b'BDC':
     d=a[1];d=d if isinstance(d,dict) else res['/Properties'][d].get_object()
     if '/MCID'in d:context=(*context,int(d['/MCID']))
     if '/ActualText'in d:
      assert active is None,'nested ActualText'
      active=len(scopes);scopes.append(dict(text=str(d['/ActualText']),mcids=context,anchors=[]))
   elif op==b'EMC':context,active=marks.pop()
   elif op==b'Do':
    x=res['/XObject'][a[0]].get_object()
    if x['/Subtype']=='/Form':
     sub=state.copy();sub['ctm']=mul(state['ctm'],tuple(map(float,x.get('/Matrix',IDENTITY))));visit(x,x.get('/Resources',res),sub,context,active,depth+1)
  assert not marks
 visit(page.get_contents(),page['/Resources'])
 return scopes
