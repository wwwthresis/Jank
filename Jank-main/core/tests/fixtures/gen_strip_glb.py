import struct, json, sys, numpy as np, math
out=sys.argv[1]
# interleaved buffer: pos(3f) + uv(2 x ushort normalized) + pad => stride 16
pos=np.array([[0,0,0],[1,0,0],[1,1,0],[0,1,0],[0.5,2,0]],np.float32)
uv=np.array([[0,0],[1,0],[1,1],[0,1],[.5,1]])
buf=b''
for p,u in zip(pos,uv):
    buf+=struct.pack('<3f',*p)+struct.pack('<2H',*[int(round(x*65535)) for x in u])
stride=16
idxs=np.array([0,1,3,2,4],np.uint32)  # triangle strip
ib=idxs.tobytes()
while len(buf)%4: buf+=b'\0'
ioff=len(buf); buf+=ib
# node: parent translated (10,0,0) rotated 90deg about Z, child scale 2
s=math.sin(math.pi/4); c=math.cos(math.pi/4)
j={"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"translation":[10,0,0],"rotation":[0,0,s,c],"children":[1]},{"scale":[2,2,2],"mesh":0}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0,"TEXCOORD_0":1},"indices":2,"mode":5}]}],
"accessors":[{"bufferView":0,"componentType":5126,"count":5,"type":"VEC3","min":[0,0,0],"max":[1,2,0]},
 {"bufferView":0,"byteOffset":12,"componentType":5123,"normalized":True,"count":5,"type":"VEC2"},
 {"bufferView":1,"componentType":5125,"count":5,"type":"SCALAR"}],
"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":len(pos)*stride,"byteStride":stride},{"buffer":0,"byteOffset":ioff,"byteLength":len(ib)}],
"buffers":[{"byteLength":len(buf)}]}
js=json.dumps(j).encode()
while len(js)%4: js+=b' '
glb=b'glTF'+struct.pack('<II',2,12+8+len(js)+8+len(buf))+struct.pack('<II',len(js),0x4E4F534A)+js+struct.pack('<II',len(buf),0x004E4942)+buf
open(out+'/strip.glb','wb').write(glb)
# expected world positions: p*2 then rotate 90deg about Z (x,y)->(-y,x), then +10 x
exp=[]
for p in pos:
    q=p*2; q=np.array([-q[1],q[0],q[2]])+np.array([10,0,0]); exp.append(q.tolist())
json.dump({"pos":exp,"tris":[[0,1,3],[3,1,2],[3,2,4]]},open(out+'/strip.expected.json','w'))
