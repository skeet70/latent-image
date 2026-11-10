module Replacement.RtosTaskRecords.System (systemPlans, systemPlansForPackaged) where

import Control.Monad (forM, unless)
import qualified Data.ByteString as B
import Data.List (isInfixOf)
import qualified Data.Map.Strict as M
import Data.Word (Word32)
import Emulation.SchedulerPairPlan
import qualified FujiAF.UpdateContainer as Update
import Replacement.RtosOwnFirst.Regions (Domain (..), Extent (..), GuestAddress (..), domains)
import qualified Replacement.RtosPlatform.Aggregate as A
import qualified Replacement.RtosPlatform.Artifact as F
import qualified Replacement.RtosPlatform.Union as U

systemPlans :: U.Union -> F.CanonicalSource -> F.Linked -> B.ByteString -> Either String (Plan,Plan)
systemPlans union source linked candidate = do
  either (Left . show) Right (F.requirePinned source)
  admitted<-either (Left . show) Right (F.admit union source linked)
  staged<-either (Left . show) Right (F.stage admitted)
  unless (candidate==F.stagedDat staged) (Left "candidate is not the independently restaged aggregate DAT")
  systemPlansForPackaged source staged candidate

systemPlansForPackaged :: F.CanonicalSource -> F.Staged -> B.ByteString -> Either String (Plan,Plan)
systemPlansForPackaged source staged candidate = do
  either (Left . show) Right (F.requirePinned source)
  carried<-either (Left . show) Right (F.canonicalSource candidate (F.stagedLoader staged DomainA) (F.stagedLoader staged DomainB))
  unless (F.sourceLoaders carried==F.stagedLoaders staged) (Left "final package loaders differ from the proved platform stage")
  let admitted=F.stagedAdmitted staged
  catalog<-catalogFor (F.admittedUnion admitted) (F.admittedLinked admitted) staged
  stock<-derivePlan Stock (F.sourceDat source) [catalog {catalogProbes=[]}] (F.sourceLoaders source)
  owned<-derivePlan Owned candidate [catalog] (F.sourceLoaders carried)
  stockArmings<-loaderArmings source
  ownedArmings<-loaderArmings carried
  pure (stock {planArmingEntries=stockArmings},owned {planArmingEntries=ownedArmings})

loaderArmings :: F.CanonicalSource -> Either String ArmingEntries
loaderArmings source = do
  primary<-entry DomainA
  secondary<-entry DomainB
  armingEntries primary secondary
 where
  entry domain = do
    loader<-maybe (Left "missing loader arming header") Right (M.lookup domain (F.sourceLoaders source))
    unless (B.length loader>=16) (Left "loader arming header is truncated")
    pure (Address (Update.word32At loader 12))

catalogFor :: U.Union -> F.Linked -> F.Staged -> Either String Catalog
catalogFor union linked staged = do
  perDomain<-forM domains $ \domain->do
    let table=U.domainMap union domain
        entries=[entry | entry<-U.mapEntries table,U.entryGroup entry==U.TaskRecords]
        point entry=case U.entryName entry of "create"->Right CreateTaskRecord; "delete"->Right DeleteTaskRecord; _->Left "unknown task-record entry"
        loader=F.stagedLoader staged domain
        placements=if domain==DomainA then F.linkedPlacementsA linked else F.linkedPlacementsB linked
        symbolsBytes=if domain==DomainA then F.linkedSymbolsA linked else F.linkedSymbolsB linked
    symbols<-A.parseAggregateSymbols symbolsBytes
    let reset=[symbol | symbol<-symbols,"reset_task" `isInfixOf` A.symbolName symbol,"xm1_rtos_task_records_abi" `isInfixOf` A.symbolName symbol]
    helper<-case reset of [symbol]->Right symbol; _->Left "task reset helper is not unique"
    unless (any (\(address,bytes)->address<=A.symbolAddress helper && A.symbolAddress helper+ A.symbolSize helper<=address+fromIntegral bytes) placements)
      (Left "task reset helper is outside the allocated placement")
    let readWord address=sum [fromIntegral byte * 256^index | (index,byte)<-zip [0::Int ..] (B.unpack (B.take 4 (B.drop (fromIntegral (address-base domain)) loader)))] :: Word32
        barriers=[address | address<-[A.symbolAddress helper,A.symbolAddress helper+4..A.symbolAddress helper+A.symbolSize helper-4],readWord address==0xf57ff05f]
    barrier<-case barriers of [address]->Right address; _->Left "task publication helper has no unique emitted DMB SY"
    planned<-forM entries $ \entry->do
      semantic<-TaskRecordSemantic <$> point entry
      let GuestAddress address=U.entryAddress entry
      pure (Entry domain semantic (Address address))
    unless (length planned==2) (Left "both task-record entries are required")
    let windows=Window domain (Address (base domain+0x2560)) (Address (base domain+0x25c8)):
          [Window domain (Address lo) (Address hi) | U.Carrier _ (Extent (GuestAddress lo) (GuestAddress hi))<-U.mapCarriers table]
    pure (planned,windows,[Probe domain (Address barrier) PublicationBarrier (Register 0)])
  pure (Catalog (U.unionFill union) (concat [entries | (entries,_,_)<-perDomain]) (concat [windows | (_,windows,_)<-perDomain]) (concat [probes | (_,_,probes)<-perDomain]))
