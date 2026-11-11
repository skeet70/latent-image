module FontRasterHandlersScenarios where

import Data.Bits ((.&.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Int (Int32)
import Data.Word (Word8, Word16, Word32)
import PipelineModel
import Scenarios

fontRasterHandlerEntries :: [(Word32, String)]
fontRasterHandlerEntries =
 [(0x11a2ae0,"vm_large_scale"),(0x11a2b1c,"vm_set_scale"),(0x11a2b90,"vm_finish_3"),(0x11a2bf4,"vm_finish_4")
 ,(0x11a2c58,"vm_copy_x"),(0x11a2c9c,"vm_copy_y"),(0x11a2cd8,"vm_capture_bounds"),(0x11a2d74,"vm_capture_crossed_bounds")
 ,(0x11a2e10,"vm_add_right_x"),(0x11a2e6c,"vm_subtract_left_x"),(0x11a2ec4,"vm_add_left_to_y"),(0x11a2f14,"vm_subtract_lower_y")
 ,(0x11a2f68,"vm_no_op_first"),(0x11a2f80,"vm_no_op_second"),(0x11a2f98,"vm_refresh_scales")
 ,(0x11a3700,"vm_subtract_bottom_y"),(0x11a3758,"vm_add_top_y"),(0x11a37ac,"vm_subtract_left_y"),(0x11a380c,"vm_add_right_y")
 ,(0x11a386c,"vm_scale_subtract_left"),(0x11a38d4,"vm_scale_add_middle"),(0x11a3938,"vm_scale_subtract_middle"),(0x11a39a4,"vm_scale_add_right")
 ,(0x11a3a14,"vm_scale_subtract_bottom"),(0x11a3a8c,"vm_scale_add_top"),(0x11a3b00,"vm_scale_subtract_x"),(0x11a3b80,"vm_scale_add_x")
 ,(0x11a5438,"vm_finish_curve"),(0x11a5470,"vm_stored_line"),(0x11a5524,"vm_short_curve"),(0x11a5704,"vm_short_curve_to")
 ,(0x11a58c8,"vm_rotated_curve"),(0x11a5ab8,"vm_rotated_curve_to"),(0x11a5c94,"vm_short_rotated_start"),(0x11a5f34,"vm_short_segment")
 ,(0x11a60f8,"vm_short_axis_start"),(0x11a6360,"vm_translate_short"),(0x11a63f0,"vm_short_line_to"),(0x11a651c,"vm_rotate_short")
 ,(0x11a65b4,"vm_rotated_line_to"),(0x11a6718,"vm_rotated_segment"),(0x11a6910,"vm_byte_curve"),(0x11a6b08,"vm_byte_curve_to")
 ,(0x11a6ce4,"vm_byte_axis_close_cross_basis"),(0x11a6f5c,"vm_byte_axis_close_normal_basis")
 ,(0x11a71d4,"vm_byte_rotated_close_cross_basis"),(0x11a7468,"vm_byte_rotated_close_normal_basis")
 ,(0x11a76fc,"vm_byte_rotated_close"),(0x11a7908,"vm_rotated_byte_curve_to"),(0x11a7b00,"vm_byte_segment")
 ,(0x11a7ce0,"vm_byte_axis_start"),(0x11a7f58,"vm_translate_byte"),(0x11a7ff8,"vm_byte_line_to"),(0x11a8134,"vm_byte_rotated_start")
 ,(0x11a8400,"vm_rotate_byte"),(0x11a84a8,"vm_rotated_byte_line_to"),(0x11a8614,"vm_byte_rotated_line"),(0x11ab010,"execute")]

fontRasterHandlersPublications :: [StagePublication]
fontRasterHandlersPublications = [StagePublication (Address entry) (StageSymbol ("xm1_pipeline_image_font_raster_" ++ name)) | (entry,name) <- fontRasterHandlerEntries]
fontRasterHandlersPublication :: Word32 -> StagePublication
fontRasterHandlersPublication entry = case lookup entry (zip (map fst fontRasterHandlerEntries) fontRasterHandlersPublications) of
 Just publication -> publication
 Nothing -> error "unpublished font raster handler"

newtype VmAddress = VmAddress Word32 deriving (Eq, Show)
newtype ProgramAddress = ProgramAddress Word32 deriving (Eq, Show)
newtype BitmapAddress = BitmapAddress Word32 deriving (Eq, Show)
newtype Coordinate = Coordinate Int32 deriving (Eq, Show)
data Position = Position Coordinate Coordinate deriving (Eq, Show)
data OperandWidth = ShortOperand | ByteOperand deriving (Eq, Show)
data PointBasis = AxisBasis | RotatedBasis deriving (Eq, Show)
data CurveTermination = CloseStored | EndRead | StartRead deriving (Eq, Show)
data RasterFixture = RasterFixture VmAddress ProgramAddress BitmapAddress deriving (Eq, Show)

rasterFixture :: RasterFixture
rasterFixture = RasterFixture (VmAddress 0x009c00b8) (ProgramAddress 0x009d2000) (BitmapAddress 0x009d0000)
vmAddress :: RasterFixture -> Word32
vmAddress (RasterFixture (VmAddress address) _ _) = address
programAddress :: RasterFixture -> Word32
programAddress (RasterFixture _ (ProgramAddress address) _) = address
bitmapAddress :: RasterFixture -> Word32
bitmapAddress (RasterFixture _ _ (BitmapAddress address)) = address

rasterHandlerInitial :: RasterFixture -> [Word8] -> [Patch]
rasterHandlerInitial fixture program =
 [Patch (Address vmContext) (B.replicate 0x9000 0), wordsAt (vmContext+0x48) [programAddress fixture]
 ,word (vm+8) vmContext,word (vm+0x3c) (fromIntegral (length program)),word (vm+0x40) 0
 ,wordsAt (vm+0x54) [8,8],word (vm+0x68) (bitmapAddress fixture),word (vm+0x6c) 1
 ,wordsAt (vm+0x74) [0x7fff,0x7fff,0,0],Patch (Address (programAddress fixture)) (B.pack program)
 ,Patch (Address (bitmapAddress fixture)) (B.replicate 64 0)]
 where vm=vmAddress fixture; vmContext=vm-0xb8

wordSigned :: Word32 -> Int32 -> Patch
wordSigned address value = word address (fromIntegral value)
halfBytes :: Int32 -> B.ByteString
halfBytes value = let half=fromIntegral value :: Word16 in B.pack [fromIntegral half,fromIntegral (half `shiftR` 8)]
halvesAt :: Word32 -> [Int32] -> Patch
halvesAt address values = Patch (Address address) (B.concat (map halfBytes values))
position :: Int32 -> Int32 -> Position
position x y = Position (Coordinate x) (Coordinate y)
positionBounds :: Position -> (Word32,Word32)
positionBounds (Position (Coordinate x) (Coordinate y)) = (fromIntegral x,fromIntegral y)
firstPosition :: [Position] -> Position
firstPosition (point:_) = point
firstPosition [] = error "a raster fixture has no points"
positionRecord :: Position -> [Int32]
positionRecord (Position (Coordinate x) (Coordinate y)) = [y,x]

fontRasterHandlersScenarios :: B.ByteString -> (StagePublication -> Address) -> [StageScenario]
fontRasterHandlersScenarios _ _ = scaleCases ++ setupCases ++ arithmeticCases ++ pointCases ++ curveCases ++ finishCases ++ programCases ++ secondarySetupCases
 where
  fixture=rasterFixture
  vm=vmAddress fixture
  make name entry arguments initial final result = StageScenario (StageCaseName name) (fontRasterHandlersPublication entry) arguments initial final result [] []
  direct name entry program initial final result = make name entry [vm] (rasterHandlerInitial fixture program ++ directState ++ initial) final result
  directState = [word (vm+0xb4) 1,word (vm+0x8058) 1,word (vm+0x7eac) 0
   ,word (vm+0x805c) (bitmapAddress fixture),word (vm+0x8060) (bitmapAddress fixture+32)
   ,wordsAt (vm+0x7dc0) [100,300,500,700],wordsAt (vm+0x7e00) [200,400,600,800]
   ,wordsAt (vm+0x7eb4) [0,1,2,3],wordsAt (vm+0x7ec4) [40,fromIntegral (-20::Int32),0,0,512,fromIntegral (-256::Int32)]]
  scaleCases =
   [make ("font-raster-scale-"++show (entry,index,value,shift)) entry [vm,index,fromIntegral value]
    (rasterHandlerInitial fixture [] ++ [wordSigned (vm+0x7eac) shift,wordsAt (vm+0xa8) [17,23]])
    [wordSigned (vm+0x7e40+4*index) value,wordSigned (vm+0x7e60+4*index) x,wordSigned (vm+0x7e80+4*index) y] Nothing
   | entry<-[0x11a2ae0,0x11a2b1c],index<-[0,7],value<-[0,1,-129,65535],shift<-[0,8,31,32,255,256]
   ,let shifted=armLeft value shift
   ,let x=if entry==0x11a2ae0 then shifted else (23*shifted+128) `quot` 256
   ,let y=if entry==0x11a2ae0 then shifted else (17*shifted+128) `quot` 256] ++
   [direct ("font-raster-refresh-"++show small) 0x11a2f98 []
    [word (vm+0x8058) small,word (vm+0x7eac) 3,wordsAt (vm+0xa8) [17,23],wordsAt (vm+0x7e40) (map fromIntegral coefficients)]
    [wordsAt (vm+0x7e60) [fromIntegral (if small==0 then value*8 else value*8*23 `quot` 256) | value<-coefficients]
    ,wordsAt (vm+0x7e80) [fromIntegral (if small==0 then value*8 else value*8*17 `quot` 256) | value<-coefficients]] Nothing
   | small<-[0,1],let coefficients=[-129,-1,0,1,17,128,255,65535]::[Int32]]
  setupCases =
   [direct ("font-raster-copy-"++show entry) entry [] [] [word (vm+target) value] (Just 0)
   | (entry,target,value)<-[(0x11a2c58,0x7dc4,100),(0x11a2c9c,0x7e0c,600)]] ++
   [direct ("font-raster-basis-"++show crossed) entry [] []
    [wordsAt (vm+0x7ec4) [100,firstY,300,secondY,200,secondY-firstY]] (Just 0)
   | (entry,crossed,firstY,secondY)<-[(0x11a2cd8,False,600,800),(0x11a2d74,True,800,600)]] ++
   [direct ("font-raster-source-no-op-"++show entry) entry [] []
    [wordsAt (vm+0x7dc0) [100,300,500,700],wordsAt (vm+0x7e00) [200,400,600,800],word (vm+0x3c) 0] (Just 0)
   | entry<-[0x11a2f68,0x11a2f80]]
  arithmeticCases =
   [direct ("font-raster-coordinate-"++show (entry,index,amount)) entry (concat (replicate readCount (B.unpack (halfBytes amount))))
    [Patch (Address (vm+0x7dbc)) (B.singleton (fromIntegral (opcode+index)))
    ,wordsAt (vm+0x7e60) (map fromIntegral xScales),wordsAt (vm+0x7e80) (map fromIntegral yScales)]
    [wordSigned (vm+destination) (origin + sign*(if scaled then selected*amount `quot` 256 else selected))
    ,word (vm+0x40) (fromIntegral (2*readCount)),word (vm+0x3c) 0] (Just 0)
   | (entry,opcode,readCount,scaled,isY,sourceY,sign)<-arithmeticOperations,index<-[0,7],amount<-[-129,255]
   ,let xScales=[-257,-128,-1,0,1,128,257,65535]::[Int32]
   ,let yScales=[128,129,130,131,132,133,134,-511]::[Int32]
   ,let useY=opcode `elem` [0x40,0x48,0x60,0x68,0xa0,0xc0,0xc8,0xe0,0xe8]
   ,let selected=(if useY then yScales else xScales)!!fromIntegral index
   ,let origin=if isY then if sourceY then 600 else 200 else 100
   ,let destination=if isY then if sourceY then 0x7e0c else 0x7e04 else 0x7dc4]
  pointCases =
   [direct ("font-raster-point-"++show (entry,width,basis)) entry (pointOperands width [pair]) []
    [wordSigned (vm+0x7dc0) x,wordSigned (vm+0x7e00) y,word (vm+0x40) (fromIntegral (length (pointOperands width [pair]))),word (vm+0x3c) 0] (Just 0)
   | (entry,width,basis)<-[(0x11a6360,ShortOperand,AxisBasis),(0x11a651c,ShortOperand,RotatedBasis),(0x11a7f58,ByteOperand,AxisBasis),(0x11a8400,ByteOperand,RotatedBasis)]
   ,let pair=if width==ShortOperand then (64,-128) else (255,128)
   ,let Position (Coordinate x) (Coordinate y)=transformPoint width basis (position 40 (-20)) (position 512 (-256)) pair]
  curveCases = concatMap (curveFixture direct vm fixture) curveOperations ++ concatMap (lineFixture direct vm fixture) lineOperations
  finishCases =
   [direct ("font-raster-finish-"++show (entry,mode)) entry []
    [word (vm+0xb4) mode,word (vm+0x7eac) 3,wordsAt (vm+0x7ef8) [fromIntegral (-257::Int32),0,511]]
    (if mode==0 then [halvesAt (bitmapAddress fixture) [-33],halvesAt (bitmapAddress fixture+32) [63],word (vm+0x805c) (bitmapAddress fixture+2),word (vm+0x8060) (bitmapAddress fixture+34)] else []) (Just result)
   | (entry,result)<-[(0x11a2b90,0x8003),(0x11a2bf4,0x8004)],mode<-[0,1,2]] ++
   [direct "font-raster-finish-curve-rewinds-one-byte" 0x11a5438 [0xaa,0xbb]
    [word (vm+0x40) 1,wordsAt (vm+0x7edc) [77,20,30,40,50,60]]
    [word (vm+0x3c) 3,word (vm+0x40) 0,word (vm+0x7edc) 0xffffffff,halvesAt (bitmapAddress fixture+32) [20,-1,40,30,60,50]] (Just 0x8004)]
  programCases =
   [make ("font-raster-program-invalid-opcode-"++show opcode) 0x11ab010 [vm,2]
    (rasterHandlerInitial fixture [opcode,0x38])
    [word (vm+0x3c) 0,word (vm+0x40) 2,wordsAt (vm+0x7eb4) [3,8,3,8]] (Just 0xfffffff9)
   | opcode<-[0..255],not (supportedOpcode opcode)] ++
   [make ("font-raster-program-rectangle-"++show (mode,style)) 0x11ab010 [vm,mode]
    (rasterHandlerInitial fixture rectangleProgram ++ [word (vm+0x5c) style,Patch (Address (bitmapAddress fixture)) (B.replicate 8 (if style==1 then 255 else 0))])
    ([Patch (Address (bitmapAddress fixture)) (B.replicate 8 (if style==1 then 0 else 255)),word (vm+0x3c) 0,word (vm+0x40) 14,word (vm+0xb8) 8]
     ++ if mode==0 then [word (vm+0x804c) 0xffffffff,word (vm+0x805c) (vm+0xbc),word (vm+0x8060) (vm+0x32bc)] else []) (Just 0)
   | mode<-[0,2],style<-[0,1]] ++
   [make "font-raster-program-scale-and-coordinate" 0x11ab010 [vm,1]
    (rasterHandlerInitial fixture [0x20,0x01,128,0,0,0,0x71,0] ++ [word (vm+0x30) 2,Patch (Address (vm+0x34)) (B.pack [16,32])])
    [word (vm+0x7dc4) 64,word (vm+0x40) 8,word (vm+0x3c) 0] (Just 0)] ++
   [make ("font-raster-program-arithmetic-decode-"++show (opcode,index)) 0x11ab010 [vm,1]
    (rasterHandlerInitial fixture program ++ [word (vm+0x58) 16,word (vm+0x30) 8,Patch (Address (vm+0x34)) (B.pack [1,3,5,7,9,11,13,15])])
    [wordSigned (vm+if isY then 0x7e04 else 0x7dc4) result,word (vm+0x40) (fromIntegral (length program)),word (vm+0x3c) 0] (Just 0)
   | (_,opcode,readCount,scaled,isY,_,sign)<-arithmeticOperations,index<-[0..7]
   ,let program=[fromIntegral (opcode+index),0x01] ++ concat (replicate readCount [128,0]) ++ [0x71,0]
   ,let useY=opcode `elem` [0x40,0x48,0x60,0x68,0xa0,0xc0,0xc8,0xe0,0xe8]
   ,let coefficient=fromIntegral ((if useY then 8 else 16)*(1+2*index))::Int32
   ,let result=sign*(if scaled then coefficient `quot` 2 else coefficient)] ++
   [make ("font-raster-program-run-table-capacity-"++show (mode,capacity)) 0x11ab010 [vm,mode]
    (rasterHandlerInitial fixture rectangleProgram ++ [word (vm+0x84) 1] ++ runTableInitial fixture (RunTableStorage storage (RunRows 8) (RunCapacity capacity)))
    [halvesAt storage ([0..fromIntegral used-1] ++ replicate (8-fromIntegral used) (-1))
    ,halvesAt (storage+16) ([0..fromIntegral used-1] ++ replicate (8-fromIntegral used) (-1))
    ,halvesAt (storage+32) (concat (replicate (fromIntegral used) [0,7,-1]))
    ,halvesAt (vm-0xb8+0x83b0+30) [fromIntegral used]
    ,Patch (Address (bitmapAddress fixture)) (B.replicate 8 0),word (vm+0x3c) 0,word (vm+0x40) 14]
    (Just (if capacity<8 then 0xfffffffc else 0))
   | mode<-[0,2],capacity<-[0,7,8],let used=min capacity 8,let storage=bitmapAddress fixture+0x400] ++
   [make ("font-raster-program-empty-grid-"++show height) 0x11ab010 [vm,2]
    (rasterHandlerInitial fixture [] ++ [word (vm+0x58) height,halvesAt (vm+0xbc) (replicate 8 (-123))])
    [halvesAt (vm+0xbc) (replicate (fromIntegral (height `quot` 2)*2) 8 ++ [if height .&. 2/=0 then 8 else -123 | odd height]),word (vm+0x8064) height] (Just 0)
   | height<-[1,2,3,7,8]]
   ++ [make "font-raster-program-f0-reconsumes-next-opcode" 0x11ab010 [vm,1]
    (rasterHandlerInitial fixture [0xf0,0x71,0])
    [word (vm+0x3c) 0,word (vm+0x40) 3,halvesAt (vm+0x32bc) [0,-1,0,0,0,0],word (vm+0x8060) (vm+0x32c8)] (Just 0)]
  secondarySetupCases =
   [make ("font-raster-program-secondary-setup-"++show (width,height,oversample)) 0x11ab010 [vm,1]
    (rasterHandlerInitial fixture [] ++
     [wordsAt (vm+0x54) [width,height],word (vm+0xa4) oversample,word (vm+0x1c) 2
     ,halvesAt (vm+0xc) (map fromIntegral values),Patch (Address (vm+0x20)) (B.pack [1,2,255,254])
     ,wordsAt (vm+0x7e40) (replicate 8 777),wordsAt (vm+0x7e60) (replicate 8 888),wordsAt (vm+0x7e80) (replicate 8 999)])
    [wordsAt (vm+0x7e40) [if value==0 then 777 else value | value<-values]
    ,wordsAt (vm+0x7e60) [if value==0 then 888 else fromIntegral (scaled scaleY value) | value<-values]
    ,wordsAt (vm+0x7e80) [if value==0 then 999 else fromIntegral (scaled scaleX value) | value<-values]
    ,wordsAt (vm+0x7dc0) (map (fromIntegral . scaled scaleY) [1,255])
    ,wordsAt (vm+0x7e00) (map (fromIntegral . scaled scaleX) [2,254]),wordSigned (vm+0x7eac) shift] (Just 0)
   | (width,height)<-[(5,7),(128,129)],oversample<-[0,1]
   ,let values=[0,1,65408,3,0,5,0,65535]::[Word32]
   ,let multiplier=if oversample==0 then 1 else 8
   ,let scaleX=fromIntegral (width*multiplier)::Int32
   ,let scaleY=fromIntegral (height*multiplier)::Int32
   ,let small=max scaleX scaleY<128
   ,let shift=if small then 8 else 3
   ,let scaled :: Int32 -> Word32 -> Int32
        scaled dimension value=let raw=armLeft (fromIntegral value) shift in if small then (dimension*raw+128) `quot` 256 else raw]

armLeft :: Int32 -> Int32 -> Int32
armLeft value shift = if amount>=32 then 0 else value `shiftL` fromIntegral amount where amount=shift .&. 255

arithmeticOperations :: [(Word32, Word32, Int, Bool, Bool, Bool, Int32)]
arithmeticOperations =
 [(0x11a3b80,0x20,2,True,False,False,1),(0x11a3b00,0x28,2,True,False,False,-1)
 ,(0x11a3a8c,0x40,2,True,True,True,1),(0x11a3a14,0x48,2,True,True,True,-1)
 ,(0x11a39a4,0x50,1,True,False,False,1),(0x11a3938,0x58,1,True,False,False,-1)
 ,(0x11a38d4,0x60,1,True,True,False,1),(0x11a386c,0x68,1,True,True,False,-1)
 ,(0x11a380c,0xa0,1,False,False,False,1),(0x11a37ac,0xa8,1,False,False,False,-1)
 ,(0x11a3758,0xc0,1,False,True,True,1),(0x11a3700,0xc8,1,False,True,True,-1)
 ,(0x11a2e10,0xd0,0,False,False,False,1),(0x11a2e6c,0xd8,0,False,False,False,-1)
 ,(0x11a2ec4,0xe0,0,False,True,False,1),(0x11a2f14,0xe8,0,False,True,False,-1)]

pointOperands :: OperandWidth -> [(Int32,Int32)] -> [Word8]
pointOperands width pairs = concat [case width of ShortOperand -> B.unpack (halfBytes x <> halfBytes y); ByteOperand -> [fromIntegral x,fromIntegral y] | (x,y)<-pairs]
transformPoint :: OperandWidth -> PointBasis -> Position -> Position -> (Int32,Int32) -> Position
transformPoint width basis (Position (Coordinate ox) (Coordinate oy)) (Position (Coordinate ex) (Coordinate ey)) (a,b) =
 position (ox+dx `quot` 256) (oy+dy `quot` 256)
 where
  expanded value=if width==ByteOperand && value==255 then 256 else value
  u=expanded a; v=expanded b
  (dx,dy)=if basis==AxisBasis then (ex*u,ey*v) else (ex*u-ey*v,ey*u+ex*v)

curveOperations :: [(Word32,Word8,OperandWidth,PointBasis,CurveTermination,Maybe Bool)]
curveOperations =
 [(0x11a5524,0x1b,ShortOperand,AxisBasis,CloseStored,Nothing),(0x11a5704,0x1a,ShortOperand,AxisBasis,EndRead,Nothing)
 ,(0x11a58c8,0x0b,ShortOperand,RotatedBasis,CloseStored,Nothing),(0x11a5ab8,0x0a,ShortOperand,RotatedBasis,EndRead,Nothing)
 ,(0x11a5c94,0x08,ShortOperand,RotatedBasis,StartRead,Nothing),(0x11a60f8,0x18,ShortOperand,AxisBasis,StartRead,Nothing)
 ,(0x11a6910,0x9b,ByteOperand,AxisBasis,CloseStored,Nothing),(0x11a6b08,0x9a,ByteOperand,AxisBasis,EndRead,Nothing)
 ,(0x11a6ce4,0x7e,ByteOperand,AxisBasis,CloseStored,Just True),(0x11a6f5c,0x7a,ByteOperand,AxisBasis,CloseStored,Just False)
 ,(0x11a71d4,0x7c,ByteOperand,RotatedBasis,CloseStored,Just True),(0x11a7468,0x78,ByteOperand,RotatedBasis,CloseStored,Just False)
 ,(0x11a76fc,0x8b,ByteOperand,RotatedBasis,CloseStored,Nothing),(0x11a7908,0x8a,ByteOperand,RotatedBasis,EndRead,Nothing)
 ,(0x11a7ce0,0x98,ByteOperand,AxisBasis,StartRead,Nothing),(0x11a8134,0x88,ByteOperand,RotatedBasis,StartRead,Nothing)]

curveFixture :: (String -> Word32 -> [Word8] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario) -> Word32 -> RasterFixture -> (Word32,Word8,OperandWidth,PointBasis,CurveTermination,Maybe Bool) -> [StageScenario]
curveFixture direct vm fixture (entry,opcode,width,basis,termination,crossed) =
 [direct ("font-raster-quadratic-"++show (entry,count)) entry program
  [Patch (Address (vm+0x7dbc)) (B.singleton opcode)]
  ([halvesAt (bitmapAddress fixture) (concatMap (concatMap positionRecord) records)
   ,word (vm+0x805c) (bitmapAddress fixture+fromIntegral (length records*12)),word (vm+0x40) (fromIntegral (length program)),word (vm+0x3c) 0]
   ++ case termination of CloseStored -> []; _ -> let (leftWord,rightWord)=positionBounds end in [word (vm+0x7dc4) leftWord,word (vm+0x7e0c) rightWord]) (Just 0)
 | count<-if termination==CloseStored then [2,4] else [2]
 ,let pairs=if width==ShortOperand then [(64,-128),(128,64),(-64,128)] else [(255,128),(128,64),(64,255)]
 ,let origin=case crossed of Nothing -> position 40 (-20); Just True -> position 100 800; Just False -> position 100 600
 ,let extent=case crossed of Nothing -> position 512 (-256); Just True -> position 200 (-200); Just False -> position 200 200
 ,let points=map (transformPoint width basis origin extent) pairs
 ,let start=if termination==StartRead then let Position x _=firstPosition points in Position x (Coordinate 600) else position 100 600
 ,let control=points!!if termination==StartRead then 1 else 0
 ,let end=points!!if termination==StartRead then 2 else 1
 ,let records=case termination of CloseStored -> if count==2 then [[start,control,position 300 800]] else [[start,control,end],[end,points!!2,position 300 800]]; _ -> [[start,control,end]]
 ,let payload=case termination of CloseStored -> take (fromIntegral count-1) pairs; StartRead -> take 3 pairs; EndRead -> take 2 pairs
 ,let countValue=if termination==StartRead then 3 else count
 ,let prefix=case width of ShortOperand -> B.unpack (halfBytes countValue); ByteOperand -> [fromIntegral countValue]
 ,let program=prefix ++ pointOperands width payload]

lineOperations :: [(Word32,Word8,OperandWidth,PointBasis,Bool,Bool)]
lineOperations = [(0x11a5470,3,ShortOperand,AxisBasis,False,False)
 ,(0x11a5f34,0x10,ShortOperand,AxisBasis,True,False),(0x11a63f0,0x12,ShortOperand,AxisBasis,False,True)
 ,(0x11a65b4,2,ShortOperand,RotatedBasis,False,False),(0x11a6718,0,ShortOperand,RotatedBasis,True,False)
 ,(0x11a7b00,0x90,ByteOperand,AxisBasis,True,False),(0x11a7ff8,0x92,ByteOperand,AxisBasis,False,True)
 ,(0x11a84a8,0x82,ByteOperand,RotatedBasis,False,False),(0x11a8614,0x80,ByteOperand,RotatedBasis,True,False)]

lineFixture :: (String -> Word32 -> [Word8] -> [Patch] -> [Patch] -> Maybe Word32 -> StageScenario) -> Word32 -> RasterFixture -> (Word32,Word8,OperandWidth,PointBasis,Bool,Bool) -> [StageScenario]
lineFixture direct vm fixture (entry,opcode,width,basis,segment,sameY) =
 [direct ("font-raster-line-"++show (entry,side,alias)) entry program
  [Patch (Address (vm+0x7dbc)) (B.singleton (opcode+side)),wordsAt (vm+0x7eb4) (if alias then [0,0,0,0] else [0,1,2,3])]
  [halvesAt output (concatMap positionRecord [start,start,end]),word cursor (output+12),word (vm+0x40) (fromIntegral (length program)),word (vm+0x3c) 0] (Just 0)
 | side<-[0,4],alias<-[False,True]
 ,let pairs=if width==ShortOperand then [(64,-128),(128,64)] else [(255,128),(128,64)]
 ,let points=map (transformPoint width basis (position 40 (-20)) (position 512 (-256))) pairs
 ,let start=if alias then if opcode==3 then position 100 200 else firstPosition points else if segment then let Position x _=firstPosition points in Position x (Coordinate 600) else position 100 600
 ,let readEnd=points!!if segment then 1 else 0
 ,let end=if opcode==3 then if alias then position 100 200 else position 300 800 else let Position x y=readEnd in Position x (if sameY || alias then y else Coordinate 800)
 ,let program=if opcode==3 then [] else pointOperands width (take (if segment then 2 else 1) pairs)
 ,let output=bitmapAddress fixture+if side==0 then 0 else 32
 ,let cursor=vm+if side==0 then 0x805c else 0x8060]

rectangleProgram :: [Word8]
rectangleProgram = [0x90,0x01,0,0,255,0,0x94,0x23,0,255,255,255,0x71,0]
supportedOpcode :: Word8 -> Bool
supportedOpcode opcode = opcode `elem` [0,2,3,4,6,7,8,10,11,12,14,15,16,18,20,22,24,26,27,28,30,31,0xb4,0xb5,0xf0]
 || any (\(first,lastValue)->first<=opcode && opcode<=lastValue) [(0x20,0x37),(0x40,0x71),(0x78,0x7f),(0xa0,0xaf),(0xc0,0xef)]
 || opcode `elem` [0x80,0x82,0x84,0x86,0x88,0x8a,0x8b,0x8c,0x8e,0x8f,0x90,0x92,0x94,0x96,0x98,0x9a,0x9b,0x9c,0x9e,0x9f]

newtype RunRows = RunRows Word16 deriving (Eq, Show)
newtype RunCapacity = RunCapacity Word16 deriving (Eq, Show)
data RunTableStorage = RunTableStorage Word32 RunRows RunCapacity deriving (Eq, Show)

runTableInitial :: RasterFixture -> RunTableStorage -> [Patch]
runTableInitial fixture (RunTableStorage storage (RunRows rows) (RunCapacity capacity)) =
 [wordsAt table [0,storage+4*fromIntegral rows,4*fromIntegral rows+6*fromIntegral capacity]
 ,halvesAt (table+12) [0,fromIntegral rows-1,fromIntegral rows,0]
 ,wordsAt (table+20) [storage,storage+2*fromIntegral rows]
 ,halvesAt (table+28) [fromIntegral capacity,0,-1]
 ,Patch (Address storage) (B.replicate (4*fromIntegral rows+6*fromIntegral capacity) 255)]
 where table=vmAddress fixture-0xb8+0x83b0
