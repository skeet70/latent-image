{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

-- | Source inventory of the X-M1 1.50 settings and property layer over
-- image4, image5 and the plain image: reviewed entry contracts and names,
-- StockInventory extents, a linear candidate census, and the storage field
-- of every property read from the getter and setter jump tables.
module Main (main) where

import Control.Monad (unless, when)
import Data.Aeson (Value, encode, object, withObject, (.:), (.=))
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (nub, sort)
import qualified Data.Map.Strict as M
import Data.Maybe (isJust)
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import PropertyFields
import ServiceInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

data Family = Accessor | Query | Gate | Conversion | Commit | Persistence | Validator | Recovery | Helper
  deriving (Eq, Ord, Show, Enum, Bounded)

familyLabel :: Family -> String
familyLabel family = case family of
  Accessor -> "accessor"
  Query -> "query"
  Gate -> "gate"
  Conversion -> "conversion"
  Commit -> "commit"
  Persistence -> "persistence"
  Validator -> "validator"
  Recovery -> "recovery"
  Helper -> "helper"

data Confidence = ByteRead | MenuLabel | Inferred deriving (Eq, Show, Enum, Bounded)

confidenceLabel :: Confidence -> String
confidenceLabel confidence = case confidence of
  ByteRead -> "byte-read"
  MenuLabel -> "label"
  Inferred -> "inference"

data PropertyName = PropertyName { nameProperty :: PropertyId, nameText :: String, nameSource :: String, nameConfidence :: Confidence }

parseName :: Value -> Parser PropertyName
parseName = withObject "property name" $ \o -> do
  exactKeys "property name" ["property", "name", "source", "confidence"] o
  property <- o .: "property" >>= hexWord "property id"
  confidence <- o .: "confidence" >>= \text -> case [c | c <- [minBound .. maxBound], confidenceLabel c == text] of
    [c] -> pure c
    _ -> fail ("unknown name confidence " ++ text)
  PropertyName (PropertyId property) <$> o .: "name" <*> o .: "source" <*> pure confidence

data Contracts = Contracts [Entry Family] [SharedObject] [PropertyName]

parseContracts :: Value -> Parser Contracts
parseContracts = withObject "contracts" $ \o -> do
  exactKeys "contracts" ["entries", "objects", "propertyNames"] o
  Contracts <$> (o .: "entries" >>= mapM (parseEntry familyLabel)) <*> (o .: "objects" >>= mapM parseObject) <*> (o .: "propertyNames" >>= mapM parseName)

pinned :: [(String, Word32, String)]
pinned =
  [ ("image4", 0x61c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16")
  , ("image5", 0xbf1a00, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a")
  , ("plain", 0x1100000, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d")
  ]

locValue :: Loc -> Value
locValue (Loc base terms offset bytes) = object
  [ "base" .= hex base
  , "terms" .= [object ["argument" .= n, "stride" .= hex stride] | (ArgumentIndex n, stride) <- terms]
  , "offset" .= offset, "bytes" .= bytes
  ]

extractValue :: Extract -> Value
extractValue (Extract lsb bits signed) = object ["lsb" .= lsb, "bits" .= bits, "signed" .= signed]

sourceValue :: Source -> Value
sourceValue source = case source of
  FromArgument (ArgumentIndex n) -> object ["kind" .= ("argument" :: String), "argument" .= n]
  FromField loc e -> object ["kind" .= ("field" :: String), "location" .= locValue loc, "extract" .= extractValue e]
  FromConstant c -> object ["kind" .= ("constant" :: String), "value" .= hex c]
  FromOther -> object ["kind" .= ("other" :: String)]

resultValue :: ArmResult -> Value
resultValue result = case result of
  ReturnsField loc e -> object ["kind" .= ("field" :: String), "location" .= locValue loc, "extract" .= extractValue e]
  ReturnsConstant c -> object ["kind" .= ("constant" :: String), "value" .= hex c]
  ReturnsArgument (ArgumentIndex n) -> object ["kind" .= ("argument" :: String), "argument" .= n]
  ReturnsOther -> object ["kind" .= ("other" :: String)]
  ReturnsIncoming -> object ["kind" .= ("incoming" :: String)]

storeValue :: StoreRecord -> Value
storeValue (StoreRecord loc extract source) = object ["location" .= locValue loc, "extract" .= extractValue extract, "source" .= sourceValue source]

guardValue :: Guard -> Value
guardValue (Guard subject constant cond holds) = object
  [ "subject" .= case subject of
      GuardArgument (ArgumentIndex n) -> object ["kind" .= ("argument" :: String), "argument" .= n]
      GuardDefaultRequested -> object ["kind" .= ("default-requested" :: String)]
      GuardUnmodelled -> object ["kind" .= ("unmodelled" :: String)]
  , "constant" .= fmap hex constant
  , "condition" .= case cond of
      CondEqual -> "eq"
      CondNotEqual -> "ne"
      CondHigherSame -> "hs"
      CondLower -> "lo"
      CondHigher -> "hi"
      CondLowerSame -> "ls"
      CondRaw code -> "code-" <> show code
  , "holds" .= holds
  ]

pathValue :: PathRecord -> Value
pathValue (PathRecord guards stores calls end exit) = object
  [ "guards" .= map guardValue guards
  , "stores" .= map pathStoreValue stores
  , "calls" .= map hex calls
  , "end" .= case end of
      EndReturn result -> object ["kind" .= ("return" :: String), "result" .= resultValue result]
      EndExit result -> object ["kind" .= ("exit" :: String), "result" .= resultValue result]
      EndCut reason -> object ["kind" .= ("cut" :: String), "reason" .= (case reason of StepLimit -> "step-limit"; OutsideImage -> "outside-image"; Unrecognized -> "unrecognized" :: String)]
  , "exit" .= fmap (\(ExitRecord paths complete) -> object ["paths" .= map pathValue (sort paths), "complete" .= complete]) exit
  ]
  where
    pathStoreValue stored = case stored of
      Modelled record -> object ["kind" .= ("modelled" :: String), "store" .= storeValue record]
      Unmodelled instruction -> object ["kind" .= ("unmodelled" :: String), "instruction" .= hex instruction]

-- | The field the getter returns and the setter writes from the value argument, when they agree;
-- the two functions take the bank index in different argument positions, so only the strides are compared.
storage :: Outcome -> Outcome -> Maybe (Loc, Extract)
storage getter setter =
  case ([(loc, e) | ReturnsField loc e <- outcomeReturns getter], [(place loc, e) | StoreRecord loc e (FromArgument (ArgumentIndex 1)) <- outcomeStores setter]) of
    ([(loc, e)], written) | (place loc, e) `elem` written, outcomeComplete getter, outcomeComplete setter -> Just (loc, e)
    _ -> Nothing
  where
    place (Loc base terms offset bytes) = (base, map snd terms, offset, bytes)

main :: IO ()
main = do
  args <- getArgs
  case args of
    [image4Path, image5Path, plainPath, contractPath, output] -> do
      exists <- doesPathExist output
      when exists (fail "output already exists; use a fresh evidence path")
      images <- mapM (\(path, (label, base, sha)) -> do
          bytes <- B.readFile path
          unless (digest bytes == sha) (fail (label ++ " is not canonical X-M1 1.50 at base " ++ hex base))
          pure (Image label path base bytes)) (zip [image4Path, image5Path, plainPath] pinned)
      contractBytes <- B.readFile contractPath
      contractValue <- either fail pure (JSON.decodeStrict contractBytes)
      Contracts entries objects names <- either fail pure (parseEither parseContracts contractValue)
      placed <- either fail pure (checkContracts images entries objects)
      unless (length (nub (map nameProperty names)) == length names) (fail "contract input repeats a property name")
      image4 <- maybe (fail "image4 missing") pure (imageOf images 0x61c400)
      (getterDefault, getterArms) <- either (fail . ("getter table: " ++)) pure (tableArms image4 getterTable)
      (setterDefault, setterArms) <- either (fail . ("setter table: " ++)) pure (tableArms image4 setterTable)
      unless (map armProperty getterArms == map armProperty setterArms) (fail "getter and setter tables cover different properties")
      let addresses = [address | Entry {entryAddress = EntryAddress address} <- entries]
          summary = census images addresses (const False) Nothing
          ordered = map snd (M.toAscList (M.fromList [(address, pair) | pair@(Entry {entryAddress = EntryAddress address}, _) <- placed]))
          extents = map (entryExtent summary) ordered
          named = M.fromList [(nameProperty n, n) | n <- names]
          property getterArm' setterArm' =
            let PropertyId identifier = armProperty getterArm'
                getter = getterArm image4 getterTable getterArm'
                setter = setterArm image4 setterTable setterArm'
                stored = storage getter setter
            in (identifier, getter, setter, stored, object
              [ "property" .= hex identifier
              , "name" .= fmap (\n -> object ["name" .= nameText n, "source" .= nameSource n, "confidence" .= confidenceLabel (nameConfidence n)]) (M.lookup (PropertyId identifier) named)
              , "getterArm" .= hex (armTarget getterArm'), "setterArm" .= hex (armTarget setterArm')
              , "getterShared" .= (armTarget getterArm' == getterDefault), "setterShared" .= (armTarget setterArm' == setterDefault)
              , "getter" .= object ["returns" .= map resultValue (sort (outcomeReturns getter)), "calls" .= map hex (sort (outcomeCalls getter)), "complete" .= outcomeComplete getter, "paths" .= map pathValue (sort (outcomePaths getter))]
              , "setter" .= object ["stores" .= map storeValue (sort (outcomeStores setter)), "calls" .= map hex (sort (outcomeCalls setter)), "complete" .= outcomeComplete setter, "paths" .= map pathValue (sort (outcomePaths setter))]
              , "storage" .= fmap (\(loc, e) -> object ["location" .= locValue loc, "extract" .= extractValue e]) stored
              ])
          properties = zipWith property getterArms setterArms
          count predicate = length [() | p <- properties, predicate p]
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Host-only source inventory of the settings and property layer over image4, image5 and the plain image; unknown contracts, unrecognized arms and extent refusals block a complete replacement. No guest execution." :: String)
        , "inputs" .= ([inputValue (imagePath image) (imageBytes image) | image <- images] ++ [inputValue contractPath contractBytes])
        , "extentMethod" .= ("StockInventory.deriveFunction with both loaders' saved-stack arrays over the entry's own image; no external tails admitted; fence = min(next direct BL target in any listed image, next listed entry, image end). Refusals claim no end." :: String)
        , "savedStackTaintResiduals" .= residualsValue
        , "censusMethod" .= ("Linear decode of every aligned word of the three images. directCalls are BL sites in any image; branchSites are non-link B sites outside the derived extent, or outside the candidate window when the derivation refused; literalWords are aligned words equal to the entry address; constructedSites are MOVW/MOVT pairs building the entry address; object sites are MOVW/MOVT pairs of one register within eight words, with no intervening MOVW of that register or unconditional transfer, or PC-relative LDR values inside the object. Embedded data decodes as instructions, so every site is a candidate; candidateFunction is the nearest preceding fence, not a derived containment." :: String)
        , "callerClosure" .= ("open: Thumb, computed, register-relative and copied-pointer uses, addresses built by other instruction sequences and callers outside the three images are not censused" :: String)
        , "pathMethod" .= ("Each property's getter.paths and setter.paths list every feasible interpreted path that ended, sorted. A path is feasible when its equality assumptions can all hold: no subject both equal and unequal to one constant, no argument equal to two constants, and the setter's default-request flag (r7, which is 1 exactly when argument 1 is 0x1affffff and 0 otherwise; on setter paths a comparison of argument 1 with 0x1affffff counts as a comparison of the flag with 1, and argument 1 equal to any other constant sets the flag to 0) left a value; returns, stores and calls above still cover every path. Each path gives the forks it took (subject: an incoming argument, the default-request flag, or unmodelled; the compared constant when the flags came from CMP with a constant; the condition code and whether it held), its stores in execution order (modelled with location, bit range and source, or unmodelled with the store instruction's address when its location is not modelled), its BL targets in execution order, and its end (return or shared exit with r0's value, or cut by the step limit, an address outside the image or an unrecognized instruction). A setter path ending at the shared exit 0x630200 carries exit: the exit interpreted from that path's own registers with r0 marked incoming, whose paths start with the arm path's guards, and whether that exploration completed. Paths dropped at the path limit are absent and complete is false. Forks the last comparison decides record no guard." :: String)
        , "fieldMethod" .= ("Each getter and setter jump-table arm, including the shared fallback arm, is interpreted from its target with the property id and its index as constants: getter r12 = id, r0 = id - 1, r1..r3 = arguments 1..3; setter r4 = id, r3 = id - 1, r5/r1 = argument 1, r6/r2 = argument 2, r8 = argument 3. A conditional instruction forks into executed and skipped paths unless the last comparison decides it: EQ, NE, HS, LO, HI and LS against two constants, and EQ/NE against an assumed equality of a value with a constant compared by CMP immediate or by CMP of a register holding a constant. BL clobbers r0-r3 and r12, B is followed, and a path ends at pop {..., pc}, bx lr, the setter's shared continuation 0x630200, an unrecognized instruction (recorded as an other result and an incomplete exploration), " ++ show maxSteps ++ " instructions, or when more than " ++ show maxPaths ++ " paths have started. Values track constants, including the image4 literal word a PC-relative LDR loads, arguments, linear values base + sum of stride * argument (one term per argument, strides summed mod 2^32, zero strides dropped, no terms being a constant) from MLA, MUL by a constant, ADD or SUB of an immediate, ADD of two linear values and ADD of a register shifted by LSL, loads from [rN, #imm] and [rN, rM, lsl #k] over linear values, whose location is a base, terms and offset, and UBFX, SBFX, LSR, AND-mask and UXT steps; a store records BFI-inserted bits when the stored value is the loaded location with an insertion, otherwise the full stored width. storage is set when the getter's returns contain exactly one distinct field, the setter stores that base, strides, offset, width and bit range from argument 1 (the bank index is argument 1 in the getter and argument 2 in the setter, so only the strides are compared, and storage.location carries the getter's terms), and both explorations are complete." :: String)
        , "propertyTables" .= object
            [ "getter" .= object ["function" .= hex (tableFunction getterTable), "dispatch" .= hex (tableDispatch getterTable), "default" .= hex getterDefault, "entries" .= tableEntries getterTable]
            , "setter" .= object ["function" .= hex (tableFunction setterTable), "dispatch" .= hex (tableDispatch setterTable), "default" .= hex setterDefault, "entries" .= tableEntries setterTable]
            ]
        , "propertySummary" .= object
            [ "properties" .= length properties
            , "getterShared" .= length [() | a <- getterArms, armTarget a == getterDefault]
            , "setterShared" .= length [() | a <- setterArms, armTarget a == setterDefault]
            , "storageAgreed" .= count (\(_, _, _, st, _) -> isJust st)
            , "named" .= M.size named
            ]
        , "properties" .= [v | (_, _, _, _, v) <- properties]
        , "entries" .= zipWith (entryValue familyLabel summary) (map fst ordered) extents
        , "objects" .= map (objectValue summary) objects
        , "families" .= [object ["family" .= familyLabel family, "entries" .= length [() | entry <- entries, entryFamily entry == family]] | family <- [minBound .. maxBound :: Family]]
        ]) <> "\n")
    _ -> fail "usage: settings-manifest IMAGE4 IMAGE5 PLAIN CONTRACTS_JSON OUTPUT_JSON"
